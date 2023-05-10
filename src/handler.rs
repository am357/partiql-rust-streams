use crate::{ws, Client, Clients, Result, Computations, Computation};
use serde::{Deserialize, Serialize};
use uuid::{Uuid};
use warp::{http::StatusCode, reply::json, ws::Message, Reply};
use crate::partiql::{evaluate, parse};
use partiql_value::ion::parse_ion;
use differential_dataflow::input::InputSession;
use differential_dataflow::operators::{Count};
use futures::StreamExt;
use partiql_eval::env::basic::MapBindings;
use partiql_logical::{BindingsOp, LogicalPlan};
use partiql_logical_planner::lower;
use partiql_value::{BindingsName, partiql_list, partiql_tuple, Value};
use partiql_value::Tuple;
use partiql_value::List;

#[derive(Deserialize, Debug)]
pub struct RegisterRequest {
    user_id: usize,
    computation: String,
}

#[derive(Serialize, Debug)]
pub struct RegisterResponse {
    url: String,
}

#[derive(Deserialize, Debug)]
pub struct CreateComputationRequest {
    name: String,
    query: String,
}

#[derive(Serialize, Debug)]
pub struct CreateComputationResponse {
    name: String,
}

#[derive(Deserialize, Debug, Clone)]
pub struct Event {
    computation: String,
    user_id: Option<usize>,
    data: String,
    // TODO create a custom type for eval_mode
    eval_mode: String,
}

pub async fn publish_handler(body: Event, computations: Computations, clients: Clients) -> Result<impl Reply> {
    let computation = computations.read().await.get(&body.computation).cloned();

    match computation {
        Some(c) => {
            let new_c = Computation {
                name: c.name.clone(),
                query: c.query.clone(),
                time: c.time.clone() + 1,
                logical_plan: c.logical_plan.clone(),
            };

            let size = std::env::args().nth(1).unwrap().parse().unwrap();
            let res = match body.eval_mode.as_str() {
                "default" => {
                    execute_computation(&c.logical_plan, &body.data, size).await
                }
                "diff" => {
                    timely::execute_from_args(std::env::args(), move |worker| {
                        let mut input= InputSession::new();

                        let probe = worker.dataflow(|scope| {
                            let output = input.to_collection(scope).map(|(k, _)| k).count();
                            output.probe()
                        });

                        let val = get_value(&body.data, size);
                        let val_as_list = val.coerce_to_tuple().get(&BindingsName::CaseInsensitive("data".to_string())).unwrap().clone();
                        if val_as_list.clone().is_list() {
                            for v in val_as_list.clone().into_iter() {
                                let t = v.coerce_to_tuple();
                                let a = t.get(&BindingsName::CaseInsensitive("a".to_string())).expect("'a' attribute");
                                let b = t.get(&BindingsName::CaseInsensitive("b".to_string())).expect("'a' attribute");
                                match (a, b) {
                                    (partiql_value::Value::Integer(a_int), partiql_value::Value::Integer(b_int)) => {
                                        input.insert((*a_int, *b_int));
                                    },
                                    _ => todo!()
                                }
                            }
                        }
                        input.advance_to(new_c.time);
                        input.flush();

                        for v in val_as_list.clone().into_iter() {
                            let t = v.coerce_to_tuple();
                            let a = t.get(&BindingsName::CaseInsensitive("a".to_string())).expect("'a' attribute");
                            let b = t.get(&BindingsName::CaseInsensitive("b".to_string())).expect("'a' attribute");
                            match (a, b) {
                                (partiql_value::Value::Integer(a_int), partiql_value::Value::Integer(b_int)) => {
                                    // (a_int, b_int, a_int + b_int)
                                    input.remove((*a_int, *b_int));
                                    input.insert((*a_int, *b_int + 1));
                                },
                                _ => todo!()
                            }
                        }
                        input.advance_to(new_c.time + 1);
                        input.flush();
                        while probe.less_than(&input.time()) { worker.step(); }
                        println!("{:?}\tdata loaded", worker.timer().elapsed());
                    }).expect("Computation terminated abnormally");
                    "done".to_string()
                },
                _ => todo!()
            };

            clients
            .read()
            .await
            .iter()
            .filter(|(_, client)| match body.user_id {
                Some(v) => client.user_id == v,
                None => true,
            })
            .filter(|(_, client)| client.computations.contains(&c.name))
            .for_each(|(_, client)| {
                if let Some(sender) = &client.sender {
                    // let out = format!("{{'result': {}}}", res.clone());
                    let out = format!("{{'result': 'done'}}");
                    let _ = sender.send(Ok(Message::text(&out)));
                }
            });

            computations.write().await.insert(body.computation.clone(), new_c);
            Ok(StatusCode::OK)
        }
        None => Err(warp::reject::not_found()),
    }
}

async fn execute_computation(logical_plan: &LogicalPlan<BindingsOp>, env: &str, size: usize) -> String {
    let env_as_value = get_value(env, size);
    let bindings: MapBindings<partiql_value::Value> = MapBindings::from(env_as_value);
    format!("{:?}", evaluate(logical_plan.clone(), bindings))
}

pub async fn register_handler(body: RegisterRequest, computations: Computations, clients: Clients) -> Result<impl Reply> {
    let user_id = body.user_id;
    let computation = body.computation;
    let uuid = Uuid::new_v4().as_simple().to_string();
    let comps_as_str: Vec<String> = computations.read().await.iter().map(|(name, _)| name.clone()).collect();

    match comps_as_str.contains(&computation) {
        true => {
            let client = clients.read().await.get(&uuid).cloned();
            match client {
                Some(c) => {
                    update_clients(uuid.clone(), computation.clone(), &c, clients);
                    Ok(json(&RegisterResponse {
                        url: format!("ws://127.0.0.1:8000/ws/{}", uuid),
                    }))
                },
                None => {
                    register_client(uuid.clone(), user_id, computation.clone(), clients).await;
                    Ok(json(&RegisterResponse {
                        url: format!("ws://127.0.0.1:8000/ws/{}", uuid),
                    }))
                }
            }
        }
        false => {
            Ok(json(&RegisterResponse {
                url: format!("no such computation exist"),
            }))
        }
    }

}

async fn update_clients(id: String, computation: String, client: &Client, clients: Clients) {
    let mut new_client = client.clone();
    new_client.computations.push(computation);

    clients.write().await.insert(
        id,
        new_client,
    );
}

async fn register_client(id: String, user_id: usize, computation: String, clients: Clients) {
    clients.write().await.insert(
        id,
        Client {
            user_id,
            computations: vec![computation],
            sender: None,
        },
    );
}

pub async fn unregister_handler(id: String, clients: Clients) -> Result<impl Reply> {
    clients.write().await.remove(&id);
    Ok(StatusCode::OK)
}

pub async fn ws_handler(ws: warp::ws::Ws, id: String, clients: Clients) -> Result<impl Reply> {
    let client = clients.read().await.get(&id).cloned();
    match client {
        Some(c) => Ok(ws.on_upgrade(move |socket| ws::client_connection(socket, id, clients, c))),
        None => Err(warp::reject::not_found()),
    }
}

pub async fn computation_handler(body: CreateComputationRequest, computations: Computations) -> Result<impl Reply> {
    let name = body.name;
    let query = body.query;

    create_computation(name.clone(), query, computations).await;
    Ok(json(&CreateComputationResponse {
        name
    }))
}

async fn create_computation(name: String, query: String, computations: Computations) {
    let parsed = parse(&query);
    match parsed {
        Ok(p) => {
            computations.write().await.insert(
                name.clone(),
                Computation { name: name.clone(), query: query.clone(), time: 0, logical_plan: lower(&p) }
            );
        },
        _ => todo!()
    }
}

pub async fn health_handler() -> Result<impl Reply> {
    Ok(StatusCode::OK)
}

fn get_value(data: &str, size: usize) -> Value {
    match data {
        "example" => {
            let mut bench_l = partiql_list![];
            for _ in 1..size {
                let a: i16 = rand::random::<i16>();
                let b: i16 = rand::random::<i16>();
                let t = partiql_tuple![
                    ("a", partiql_value::Value::Integer(a.into())),
                    ("b".into(), partiql_value::Value::Integer(b.into()))
                ];
                bench_l.push(t.into());
            }
            return partiql_value::Value::Tuple(Box::new(partiql_tuple![("data".into(), bench_l)]))
        }
        _ => {
            let val = parse_ion(data);
            // let v =
            return partiql_value::Value::Tuple(Box::new(val.coerce_to_tuple().clone()))
        }
    };
}
