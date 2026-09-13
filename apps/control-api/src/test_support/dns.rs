use serde_json::{Value, json};
use std::collections::HashMap;

use crate::infra::dns::Resolver;

pub async fn fixture(records: Vec<(&str, &str, Value)>) -> (Resolver, tokio::task::JoinHandle<()>) {
    let records = records
        .into_iter()
        .map(|(name, kind, value)| ((name.to_owned(), kind.to_owned()), value))
        .collect::<HashMap<_, _>>();
    let app = axum::Router::new()
        .route("/dns", axum::routing::get(lookup))
        .with_state(records);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let resolver =
        Resolver::with_endpoint(&format!("http://{}/dns", listener.local_addr().unwrap())).unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (resolver, server)
}

pub fn answer(name: &str, kind: u16, data: &str) -> Value {
    json!({"Status":0,"Answer":[{"name":name,"type":kind,"data":data}]})
}

type Records = HashMap<(String, String), Value>;

async fn lookup(
    axum::extract::State(records): axum::extract::State<Records>,
    axum::extract::Query(query): axum::extract::Query<HashMap<String, String>>,
) -> axum::Json<Value> {
    let key = (query["name"].clone(), query["type"].clone());
    axum::Json(
        records
            .get(&key)
            .cloned()
            .unwrap_or_else(|| json!({"Status": 0, "Answer": []})),
    )
}
