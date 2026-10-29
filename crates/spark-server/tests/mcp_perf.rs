//! MCP read tools answer in under 10 ms p50 on a 20,000-node file (spec budget).

mod common;

use std::time::Instant;

use common::TestServer;
use serde_json::{Value, json};

async fn call(s: &TestServer, name: &str, args: Value) -> Value {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}})
        .to_string();
    let auth = format!("Bearer {}", s.token);
    let (_, text) =
        s.http("POST", "/mcp", &[("Authorization", &auth), ("Content-Type", "application/json")], &body).await;
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(v["result"]["isError"].is_null(), "{name}: {v}");
    v["result"]["content"].as_array().unwrap().last().unwrap()["text"]
        .as_str()
        .map(|t| serde_json::from_str(t).unwrap_or(json!(t)))
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn read_tools_answer_within_budget_on_20k_nodes() {
    let s = TestServer::start().await;
    let path = s.files_dir().join("Big.spark");
    std::fs::create_dir_all(s.files_dir()).unwrap();
    let mut doc = spark_core::demo::demo_document(20_000);
    let page = doc.first_page().unwrap();
    for b in doc.children(spark_core::Parent::Page(page)).collect::<Vec<_>>() {
        spark_server::layout::ensure(&mut doc, b);
    }
    spark_core::store::Store::create(&path, &mut doc).unwrap();
    let file = path.to_string_lossy().to_string();
    let info = call(&s, "get_basic_info", json!({"fileId": file})).await;
    assert!(info["nodeCount"].as_u64().unwrap() >= 20_000);
    let id = info["fileId"].as_str().unwrap().to_string();
    let board = info["artboards"][0]["id"].as_str().unwrap().to_string();
    let card =
        call(&s, "get_children", json!({"fileId": id, "nodeId": board})).await[1]["id"].as_str().unwrap().to_string();

    let cases: Vec<(&str, Value)> = vec![
        ("get_basic_info", json!({"fileId": id})),
        ("get_selection", json!({"fileId": id})),
        ("get_node_info", json!({"fileId": id, "nodeId": card})),
        ("get_children", json!({"fileId": id, "nodeId": board})),
        ("get_tree_summary", json!({"fileId": id, "nodeId": board, "depth": 3})),
        ("get_jsx", json!({"fileId": id, "nodeId": card})),
        ("get_computed_styles", json!({"fileId": id, "nodeIds": [card, board]})),
        ("find_nodes", json!({"fileId": id, "textValue": "Item 7", "limit": 20})),
        ("get_tokens", json!({"fileId": id})),
    ];
    let mut report = Vec::new();
    for (name, args) in cases {
        let mut times = Vec::new();
        for _ in 0..30 {
            let t = Instant::now();
            call(&s, name, args.clone()).await;
            times.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        times.sort_by(f64::total_cmp);
        let p50 = times[times.len() / 2];
        report.push(format!("{name}: p50 {p50:.2} ms, p90 {:.2} ms", times[times.len() * 9 / 10]));
        assert!(p50 < 10.0, "{name} p50 {p50:.2} ms exceeds 10 ms");
    }
    println!("{}", report.join("\n"));
}
