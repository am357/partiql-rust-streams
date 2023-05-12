# `partiql-rust-streams`

## About the Project

`partiql-rust-streams` is a proof-of-concept project to experiment reactive queries and data-flow query processing using
PartiQL.

_Note: This project includes code from [zupzup](https://github.com/zupzup/warp-websockets-exampl), which is licensed under the MIT license._

## Demo

### Create a computation and publish data to subscribed users

First, we create a pub-sub model in which we define computations as PartiQL select-from-where (SFW) queries and allow clients to register to the computations. With this, we allow publishing data, targeting a Computation and pushing the results to clients over a WebSocket session. We use `partiql-lang-rust` for query evaluation. We lower the PartiQL query to a PartiQL Logical Plan during Computation creation.

```
# Define a computation
curl -X POST 'http://localhost:8000/computation' \
-H 'Content-Type: application/json' \
-d '{"name": "view-1", "query": "SELECT d.a + d.b FROM data AS d"}'

# Register user-1 to `view-1`
curl -X POST 'http://localhost:8000/register' \
-H 'Content-Type: application/json' \
-d '{ "user_id": 1, "computation": "view-1" }'

# Register user-2 to `view-1`
curl -X POST 'http://localhost:8000/register' \
-H 'Content-Type: application/json' \
-d '{ "user_id": 2, "computation": "view-1" }'

# Publish some random tuples to `view-1`
#!/bin/zsh

t="{'a': 1, 'b': 2}"

for counter in {1..1000000}
do
  RANDOM=$counter;
  a=$(echo $(( $RANDOM )))
  b=$(echo $(( $RANDOM )))
  d="[{'a': ${a}, 'b': ${b}}]"

  curl -X POST 'http://localhost:8000/publish' \
  -H 'Content-Type: application/json' \
  -d "{\"computation\": \"view-1\", \"eval_mode\": \"default\", \"data\": \"{'data': ${d}}\"}"
done
```

### Scale up the data and use data-flow computation

Data-flow computation appears to be a good fit for processing streaming data; based on this observation, the idea is to be able to define a computation using PartiQL while being able to use the data-flow representation of the PartiQL query to process the streaming data—basically to create a Dataflow computation from a PartiQL Plan (the rewrite is not implemented in this project).

For this demo we use [differential-dataflow](https://timelydataflow.github.io/differential-dataflow/introduction.html) to represent a simple computation (an SQL `COUNT` aggregate) for experimentation. About differential-dataflow from [GitHub](https://github.com/TimelyDataflow/differential-dataflow):

>Differential dataflow is a data-parallel programming framework designed to efficiently process large volumes of data and to quickly respond to arbitrary changes in input collections. You can read more in the [differential dataflow mdbook](https://timelydataflow.github.io/differential-dataflow/) and in the [differential dataflow documentation](https://docs.rs/differential-dataflow/).

```
# Define the computation
curl -X POST 'http://localhost:8000/computation' \
-H 'Content-Type: application/json' \
-d '{"name": "view-2", "query": "SELECT COUNT(d.b) FROM data AS d GROUP BY d.a"}'

# Register user-3 to `view-2`
curl -X POST 'http://localhost:8000/register' \
-H 'Content-Type: application/json' \
-d '{ "user_id": 3, "computation": "view-2" }'

# Publish 10 million tuples and evaluate using PartiQL
/Users/maymandi/ws/partiql-streams/target/release/partiql-streams 10000000

time curl -X POST 'http://localhost:8000/publish' \
-H 'Content-Type: application/json' \
-d "{\"computation\": \"view-2\", \"eval_mode\": \"default\", \"data\": \"example\"}"

# Result: curl -X POST 'http://localhost:8000/publish' -H  -d   0.00s user 0.01s system 0% cpu 32.853 total

# Publish 10 million tuples and evaluate using differential data-flow
time curl -X POST 'http://localhost:8000/publish' \
-H 'Content-Type: application/json' \
-d "{\"computation\": \"view-2\", \"eval_mode\": \"diff\", \"data\": \"example\"}"

# Result: curl -X POST 'http://localhost:8000/publish' -H  -d   0.00s user 0.01s system 0% cpu 12.578 total

# Publish 10 million tuples and evaluate using differential data-flow and then
# introduce a change and re-compute again:
time curl -X POST 'http://localhost:8000/publish' \
-H 'Content-Type: application/json' \
-d "{\"computation\": \"view-2\", \"eval_mode\": \"diff\", \"data\": \"example\"}"

# Result: curl -X POST 'http://localhost:8000/publish' -H  -d   0.00s user 0.01s system 0% cpu 13.484 total
```

## How

* [warp](https://docs.rs/warp/latest/warp/), [tokio](https://tokio.rs/), [websocket](https://docs.rs/websocket/latest/websocket/) for pub-sub
* [partiql-lang-rust](https://github.com/partiql/partiql-lang-rust) for PartiQL query evaluation
* [Differential Dataflow](https://github.com/TimelyDataflow/differential-dataflow) for evaluation using data-flow

## Wrap up

Based on the experiment, it seems like when doing two consequent identical computations with a change in input data, for 10 million records and aggregate `COUNT`, Differential Dataflow introduces ~ `1sec` latency using a single worker. This means there is a possibility of significant performance gain if we’re able to re-write PartiQL queries as Differential Dataflow computations. Having said that, during this project I faced the following issues with Differential Dataflow:

1. Distribute the load to multiple processors and workers is hard.
2. We may only be able to re-write a subset of PartiQL queries as the Differential Dataflow library may not include all the features that PartiQL requires.

## Next

Rewrite a small set of PartiQL queries to Differential Dataflow.

## References

1. Differential Dataflow: https://timelydataflow.github.io/differential-dataflow/introduction.html
2. Timely Dataflow: https://timelydataflow.github.io/timely-dataflow/chapter_0/chapter_0_3.html
3. Timely Dataflow intro video: https://www.youtube.com/watch?v=yOnPmVf4YWo
4. Paper: https://cs.stanford.edu/~matei/courses/2015/6.S897/readings/naiad.pdf
5. Materialize under the hood: https://materialize.com/blog/materialize-under-the-hood/
6. Hacker News discussion on Differential Dataflow: https://news.ycombinator.com/item?id=24837031
7. How to build a Websocket server with Rust: https://blog.logrocket.com/how-to-build-a-websocket-server-with-rust/




