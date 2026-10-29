mod common;

use common::TestServer;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn connect(s: &TestServer, token: &str) -> Result<Ws, String> {
    let req = format!("ws://127.0.0.1:{}/ui?token={token}", s.port).into_client_request().unwrap();
    tokio_tungstenite::connect_async(req).await.map(|(ws, _)| ws).map_err(|e| e.to_string())
}

/// Sends a request and waits for its reply, collecting other messages on the way.
async fn request(ws: &mut Ws, rid: u64, mut msg: Value, seen: &mut Vec<Value>) -> Value {
    msg["rid"] = json!(rid);
    ws.send(Message::Text(msg.to_string().into())).await.unwrap();
    loop {
        let m = ws.next().await.unwrap().unwrap();
        let v: Value = serde_json::from_str(m.to_text().unwrap()).unwrap();
        if v["t"] == "reply" && v["rid"] == json!(rid) {
            assert_eq!(v["ok"], json!(true), "{v}");
            return v["result"].clone();
        }
        seen.push(v);
    }
}

#[tokio::test]
async fn rejects_bad_tokens_origins_and_hosts() {
    let s = TestServer::start().await;
    assert!(connect(&s, "wrong").await.is_err());
    let (status, _) = s.http("GET", "/health", &[("Origin", "http://evil.example")], "").await;
    assert_eq!(status, 403);
    let (status, _) = s.http("GET", "/health", &[("Host", "evil.example")], "").await;
    assert_eq!(status, 403);
    let origin = format!("http://127.0.0.1:{}", s.port);
    let (status, _) = s.http("GET", "/health", &[("Origin", &origin)], "").await;
    assert_eq!(status, 200);
    let (status, body) = s.http("GET", "/.well-known/oauth-authorization-server", &[], "").await;
    assert_eq!(status, 404);
    assert!(!body.contains("error"));
}

#[tokio::test]
async fn snapshot_intents_and_patches() {
    let s = TestServer::start().await;
    let mut ws = connect(&s, &s.token).await.unwrap();
    let mut seen = Vec::new();
    let file = request(&mut ws, 1, json!({"t": "create", "name": "Proto"}), &mut seen).await;
    let id = file["id"].as_str().unwrap().to_string();
    let snap = request(&mut ws, 2, json!({"t": "open", "ref": id}), &mut seen).await;
    assert_eq!(snap["file"]["name"], "Proto");
    let (board, view) = snap["nodes"].as_object().unwrap().iter().next().unwrap();
    assert_eq!(view["css"]["width"], "1440px");
    let page = snap["page"].as_str().unwrap();

    // Measured rects come back from the UI; then a create intent produces a patch.
    ws.send(Message::Text(json!({"t": "rects", "file": id, "rects": {board: [0, 0, 1440, 1024]}}).to_string().into()))
        .await
        .unwrap();
    let created = request(&mut ws, 3, json!({"t": "do", "file": id, "cmd": {"type": "create", "kind": "rect", "parent": board, "x": 10, "y": 20, "w": 100, "h": 50}}), &mut seen).await;
    let rect = created["id"].as_str().unwrap();
    let patch = seen.iter().find(|m| m["t"] == "patch").expect("patch").clone();
    assert_eq!(patch["nodes"][rect]["css"]["left"], "10px");
    assert_eq!(patch["nodes"][rect]["parent"], board.as_str());
    assert_eq!(patch["canUndo"], true);

    // Style intents take CSS; bad values are reported, not applied.
    let r = request(&mut ws, 4, json!({"t": "do", "file": id, "cmd": {"type": "setStyle", "ids": [rect], "styles": {"backgroundColor": "#ff0000", "width": "nope"}}}), &mut seen).await;
    assert_eq!(r["ignored"].as_array().unwrap().len(), 1);

    // Undo the style change, then the create.
    request(&mut ws, 5, json!({"t": "do", "file": id, "cmd": {"type": "undo"}}), &mut seen).await;
    seen.clear();
    request(&mut ws, 6, json!({"t": "do", "file": id, "cmd": {"type": "undo"}}), &mut seen).await;
    let patch = seen.iter().find(|m| m["t"] == "patch").unwrap();
    assert_eq!(patch["nodes"][rect], Value::Null);

    // Pages are intents too.
    let added = request(&mut ws, 7, json!({"t": "do", "file": id, "cmd": {"type": "addPage"}}), &mut seen).await;
    assert_ne!(added["id"], page);

    // A second client sees the file in its recent list, and the file is on disk.
    let mut ws2 = connect(&s, &s.token).await.unwrap();
    let hello = request(&mut ws2, 1, json!({"t": "hello"}), &mut Vec::new()).await;
    assert_eq!(hello["recent"][0]["id"], id.as_str());
    assert!(s.files_dir().join("Proto.spark").exists());
}

#[tokio::test]
async fn drag_session_snaps_and_is_one_undo_step() {
    let s = TestServer::start().await;
    let mut ws = connect(&s, &s.token).await.unwrap();
    let mut seen = Vec::new();
    let id = request(&mut ws, 1, json!({"t": "create", "name": "Drag"}), &mut seen).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let snap = request(&mut ws, 2, json!({"t": "open", "ref": id}), &mut seen).await;
    let board = snap["nodes"].as_object().unwrap().keys().next().unwrap().clone();
    let mk = |x: i32| json!({"type": "create", "kind": "rect", "parent": board, "x": x, "y": 0, "w": 100, "h": 100});
    let a = request(&mut ws, 3, json!({"t": "do", "file": id, "cmd": mk(0)}), &mut seen).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let b = request(&mut ws, 4, json!({"t": "do", "file": id, "cmd": mk(300)}), &mut seen).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let rects = json!({"t": "rects", "file": id, "rects": {board.as_str(): [0, 0, 1440, 1024], a.as_str(): [0, 0, 100, 100], b.as_str(): [300, 0, 100, 100]}});
    ws.send(Message::Text(rects.to_string().into())).await.unwrap();

    request(&mut ws, 5, json!({"t": "do", "file": id, "cmd": {"type": "dragStart", "ids": [b]}}), &mut seen).await;
    // 300 + (-197) = 103: within 4px of a's right edge (100), so it snaps to 100.
    let r = request(
        &mut ws,
        6,
        json!({"t": "do", "file": id, "cmd": {"type": "drag", "dx": -197, "dy": 0, "zoom": 1}}),
        &mut seen,
    )
    .await;
    assert_eq!(r["dx"], json!(-200.0));
    assert!(!r["guides"].as_array().unwrap().is_empty());
    request(
        &mut ws,
        7,
        json!({"t": "do", "file": id, "cmd": {"type": "drag", "dx": -150, "dy": 40, "zoom": 1}}),
        &mut seen,
    )
    .await;
    request(&mut ws, 8, json!({"t": "do", "file": id, "cmd": {"type": "dragEnd"}}), &mut seen).await;
    seen.clear();
    request(&mut ws, 9, json!({"t": "do", "file": id, "cmd": {"type": "undo"}}), &mut seen).await;
    let patch = seen.iter().find(|m| m["t"] == "patch").unwrap();
    assert_eq!(patch["nodes"][b.as_str()]["css"]["left"], "300px", "one undo restores the start position");
}

#[tokio::test]
async fn pages_forbid_inline_scripts_and_large_uploads_fit() {
    let s = TestServer::start().await;
    let mut tcp = tokio::net::TcpStream::connect(("127.0.0.1", s.port)).await.unwrap();
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    tcp.write_all(
        format!("GET /health HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n", s.port).as_bytes(),
    )
    .await
    .unwrap();
    let mut head = String::new();
    tcp.read_to_string(&mut head).await.unwrap();
    assert!(head.to_ascii_lowercase().contains("content-security-policy: script-src 'self'"), "{head}");
    // A 3 MiB body gets past the size limit (and fails only on its contents).
    let auth = format!("Bearer {}", s.token);
    let (status, _) = s.http("POST", "/api/import?name=Big", &[("Authorization", &auth)], &"x".repeat(3 << 20)).await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn the_ui_page_is_revalidated_so_updates_show_up() {
    let dir = tempfile::tempdir().unwrap();
    let web = dir.path().join("web");
    std::fs::create_dir_all(&web).unwrap();
    std::fs::write(web.join("index.html"), "<!doctype html>").unwrap();
    let config = spark_server::Config::load(dir.path().join("config"), dir.path().join("files")).unwrap();
    let listener = spark_server::bind(0).await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(spark_server::serve_on(listener, Some(web), config));
    let mut tcp = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    tcp.write_all(format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut head = String::new();
    tcp.read_to_string(&mut head).await.unwrap();
    // An updated app serves a new bundle on the same port; a cached index.html would keep loading the old one.
    assert!(head.to_ascii_lowercase().contains("cache-control: no-cache"), "{head}");
}

/// Calls an MCP tool as an agent and returns its JSON result.
async fn mcp(s: &TestServer, name: &str, args: Value) -> Value {
    let body = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call", "params": {"name": name, "arguments": args}})
        .to_string();
    let auth = format!("Bearer {}", s.token);
    let headers = [
        ("Authorization", auth.as_str()),
        ("Content-Type", "application/json"),
        ("Accept", "application/json, text/event-stream"),
        ("mcp-session-id", "s1"),
    ];
    let (status, body) = s.http("POST", "/mcp", &headers, &body).await;
    assert_eq!(status, 200, "{body}");
    let v: Value = serde_json::from_str(&body).unwrap();
    assert_ne!(v["result"]["isError"], json!(true), "{name}: {v}");
    serde_json::from_str(v["result"]["content"].as_array().unwrap().last().unwrap()["text"].as_str().unwrap()).unwrap()
}

#[tokio::test]
async fn an_agent_moving_an_artboard_moves_its_reported_position() {
    let s = TestServer::start().await;
    let file = mcp(&s, "create_file", json!({"name": "Arrange"})).await;
    let id = file["fileId"].as_str().unwrap().to_string();
    let board =
        mcp(&s, "create_artboard", json!({"fileId": id, "name": "A", "styles": {"width": "400px", "height": "300px"}}))
            .await;
    let board = board["id"].as_str().unwrap().to_string();
    let w = mcp(
        &s,
        "write_html",
        json!({"fileId": id, "targetNodeId": board, "html": "<div style=\"width:100px;height:50px\"></div>"}),
    )
    .await;
    let child = w["created"][0]["id"].as_str().unwrap().to_string();
    mcp(
        &s,
        "update_styles",
        json!({"fileId": id, "updates": [{"nodeIds": [board], "styles": {"left": "0px", "top": "0px"}}]}),
    )
    .await;

    // The UI lays the artboard out and reports where everything is.
    let mut ws = connect(&s, &s.token).await.unwrap();
    let mut seen = Vec::new();
    request(&mut ws, 1, json!({"t": "open", "ref": id}), &mut seen).await;
    let rects = json!({"t": "rects", "file": id, "rects": {board.as_str(): [0, 0, 400, 300], child.as_str(): [10, 20, 100, 50]}});
    ws.send(Message::Text(rects.to_string().into())).await.unwrap();
    let world = |v: &Value| (v["worldX"].as_f64(), v["worldY"].as_f64());
    for _ in 0..50 {
        if world(&mcp(&s, "get_node_info", json!({"fileId": id, "nodeId": child})).await) == (Some(10.0), Some(20.0)) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }

    // An agent moves the artboard the way it would in CSS. When the artboard is a level-of-detail
    // shell (zoomed out or off screen) the UI doesn't re-measure it, so the server has to follow.
    let r = mcp(
        &s,
        "update_styles",
        json!({"fileId": id, "updates": [{"nodeIds": [board], "styles": {"position": "absolute", "left": "3000px", "top": "1200px"}}]}),
    )
    .await;
    assert_eq!(r["ignoredStyles"], json!([]));
    assert_eq!(
        world(&mcp(&s, "get_node_info", json!({"fileId": id, "nodeId": board})).await),
        (Some(3000.0), Some(1200.0))
    );
    assert_eq!(
        world(&mcp(&s, "get_node_info", json!({"fileId": id, "nodeId": child})).await),
        (Some(3010.0), Some(1220.0))
    );
}
