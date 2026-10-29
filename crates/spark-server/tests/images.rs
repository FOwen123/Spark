//! Image generation against a mock HTTP server that speaks each provider's API shape.

mod common;

use axum::{
    Json, Router,
    extract::Path,
    routing::{get, post},
};
use base64::Engine;
use common::TestServer;
use serde_json::{Value, json};

fn png() -> Vec<u8> {
    let mut out = Vec::new();
    image::RgbaImage::from_pixel(4, 2, image::Rgba([10, 20, 30, 255]))
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

fn b64() -> String {
    base64::engine::general_purpose::STANDARD.encode(png())
}

async fn mock() -> u16 {
    let app = Router::new()
        .route("/v1/images/generations", post(|| async { Json(json!({"data": [{"b64_json": b64()}]})) }))
        .route("/v1beta/models/{m}", post(|| async { Json(json!({"candidates": [{"content": {"parts": [{"text": "here"}, {"inlineData": {"mimeType": "image/png", "data": b64()}}]}}]})) }))
        .route("/sdapi/v1/txt2img", post(|| async { Json(json!({"images": [b64()]})) }))
        .route("/fal-ai/flux/schnell", post(|| async { Json(json!({"images": [{"url": format!("data:image/png;base64,{}", b64())}]})) }))
        .route("/api/v1/chat/completions", post(|| async { Json(json!({"choices": [{"message": {"images": [{"image_url": {"url": format!("data:image/png;base64,{}", b64())}}]}}]})) }))
        .route("/v1/models/{o}/{m}/predictions", post(|| async { Json(json!({"status": "succeeded", "output": [format!("data:image/png;base64,{}", b64())]})) }))
        .route("/prompt", post(|| async { Json(json!({"prompt_id": "p1"})) }))
        .route("/history/{id}", get(|Path(id): Path<String>| async move { Json(json!({id: {"outputs": {"7": {"images": [{"filename": "a.png", "subfolder": "", "type": "output"}]}}}})) }))
        .route("/view", get(|| async { png() }));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(l, app).await });
    port
}

impl TestServer {
    async fn call(&self, name: &str, args: Value) -> (bool, Value) {
        let body =
            json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}})
                .to_string();
        let auth = format!("Bearer {}", self.token);
        let (_, text) =
            self.http("POST", "/mcp", &[("Authorization", &auth), ("Content-Type", "application/json")], &body).await;
        let v: Value = serde_json::from_str(&text).unwrap_or_else(|_| panic!("{name}: {text}"));
        assert!(v["result"].is_object(), "{name}: {v}");
        let err = v["result"]["isError"].as_bool().unwrap_or(false);
        let last =
            v["result"]["content"].as_array().unwrap().last().unwrap()["text"].as_str().unwrap_or("null").to_string();
        (err, serde_json::from_str(&last).unwrap_or(Value::String(last)))
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn providers_fill_placeholders_and_the_guard_rejects_batches() {
    let s = TestServer::start().await;
    // SAFETY: set before any provider code reads it; this test binary has a single test.
    unsafe { std::env::set_var("SPARK_KEYRING_FILE", s.dir.path().join("keys.json")) };
    let mock = mock().await;
    let mut settings = spark_server::settings::Settings::default();
    for p in spark_server::generate::PROVIDERS {
        settings.images.providers.insert(
            p.id().into(),
            spark_server::settings::ProviderSettings {
                enabled: true,
                base_url: Some(format!("http://127.0.0.1:{mock}")),
                ..Default::default()
            },
        );
        if p.needs_key() {
            spark_server::secrets::set(p.id(), "test-key").unwrap();
        }
    }
    let gemini = settings.images.providers.get_mut("gemini").unwrap();
    gemini.model = Some("m".into());
    settings.save(&spark_server::Config::load(s.dir.path().join("config"), s.files_dir()).unwrap()).unwrap();

    let (_, file) = s.call("create_file", json!({"name": "Gen"})).await;
    let id = file["fileId"].as_str().unwrap().to_string();
    let (_, info) = s.call("get_basic_info", json!({"fileId": id})).await;
    let board = info["artboards"][0]["id"].as_str().unwrap().to_string();

    let providers = ["openai", "gemini", "fal", "replicate", "openrouter", "comfyui", "automatic1111"];
    let html: String = providers
        .iter()
        .map(|p| format!(r#"<img layer-name="{p}" src="spark-gen://{p}?prompt=a%20blue%20square&aspect_ratio=2:1">"#))
        .collect();
    let (err, out) = s.call("write_html", json!({"fileId": id, "targetNodeId": board, "html": html})).await;
    assert!(!err, "{out}");
    let created = out["created"].as_array().unwrap().clone();
    assert_eq!(created.len(), providers.len());
    for node in &created {
        let nid = node["id"].as_str().unwrap();
        let mut status = String::new();
        for _ in 0..100 {
            let (_, info) = s.call("get_node_info", json!({"fileId": id, "nodeId": nid})).await;
            status = info["imageGeneration"]["status"].as_str().unwrap_or("").to_string();
            if status != "processing" {
                assert_eq!(status, "ready", "{} → {info}", node["name"]);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert_eq!(status, "ready", "{}", node["name"]);
        let (_, css) = s.call("get_computed_styles", json!({"fileId": id, "nodeIds": [nid]})).await;
        assert!(css["styles"][nid]["backgroundImage"].as_str().unwrap().contains("/assets/"), "{css}");
    }

    // More images than the per-call cap: the whole call is rejected and nothing is written.
    let mut tight = spark_server::settings::Settings::load(
        &spark_server::Config::load(s.dir.path().join("config"), s.files_dir()).unwrap(),
    );
    tight.images.per_call_cap = 1;
    tight.save(&spark_server::Config::load(s.dir.path().join("config"), s.files_dir()).unwrap()).unwrap();
    let two = r#"<img src="spark-gen://openai?prompt=a"><img src="spark-gen://openai?prompt=b">"#;
    let (err, out) = s.call("write_html", json!({"fileId": id, "targetNodeId": board, "html": two})).await;
    assert!(err && out.as_str().unwrap().contains("limit is 1"), "{out}");
    let (_, info) = s.call("get_basic_info", json!({"fileId": id})).await;
    assert_eq!(info["nodeCount"], 1 + providers.len() as u64);

    // Agent requests aren't generated by Spark: they wait for fulfill_image_request.
    let (_, out) = s
        .call(
            "write_html",
            json!({"fileId": id, "targetNodeId": board, "html": r#"<img src="spark-gen://agent?prompt=a%20cat">"#}),
        )
        .await;
    let nid = out["created"][0]["id"].as_str().unwrap().to_string();
    let (_, reqs) = s.call("list_image_requests", json!({"fileId": id})).await;
    assert_eq!(reqs["count"], 1);
    assert_eq!(reqs["requests"][0]["prompt"], "a cat");
    let rid = reqs["requests"][0]["requestId"].as_str().unwrap().to_string();
    let (err, out) = s.call("fulfill_image_request", json!({"fileId": id, "requestId": rid, "base64": b64()})).await;
    assert!(!err, "{out}");
    let (_, info) = s.call("get_node_info", json!({"fileId": id, "nodeId": nid})).await;
    assert_eq!(info["imageGeneration"]["status"], "ready");
    let (_, reqs) = s.call("list_image_requests", json!({"fileId": id})).await;
    assert_eq!(reqs["count"], 0);
}
