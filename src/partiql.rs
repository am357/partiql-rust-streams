use partiql_parser::{Parsed, Parser, ParserResult};
use partiql_logical as logical;
use partiql_logical_planner::lower;
use partiql_eval as eval;
use partiql_eval::env::basic::MapBindings;
use partiql_value::ion::parse_ion;

pub fn eval_as_string(statement: &str, env: &str) -> String {
    let parsed = parse(statement);
    match parsed {
        Ok(p) => {
            format!("{:?}", eval(&p, env))
        },
        Err(e) => {
            format!("{:?}", e)
        }
    }
}

pub(crate) fn parse(statement: &str) -> ParserResult {
    Parser::default().parse(statement)
}

fn eval(p: &Parsed, env: &str) -> partiql_value::Value {
    let lowered = lower(p);
    let env_as_value = parse_ion(env);
    let bindings: MapBindings<partiql_value::Value> = MapBindings::from(env_as_value);
    evaluate(lowered, bindings)
}


pub fn evaluate(
    logical: logical::LogicalPlan<logical::BindingsOp>,
    bindings: MapBindings<partiql_value::Value>,
) -> partiql_value::Value {
    let planner = eval::plan::EvaluatorPlanner;

    let mut plan = planner.compile(&logical);

    if let Ok(out) = plan.execute_mut(bindings) {
        out.result
    } else {
        partiql_value::Value::Missing
    }
}