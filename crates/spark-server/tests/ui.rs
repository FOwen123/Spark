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
    // Font routes take no token, so pages from other sites are turned away by the browser's own label.
    let (status, _) = s.http("GET", "/fonts/file/a.woff2", &[("Sec-Fetch-Site", "cross-site")], "").await;
    assert_eq!(status, 403);
    let (status, _) = s.http("GET", "/fonts/css", &[("Sec-Fetch-Site", "same-site")], "").await;
    assert_eq!(status, 403);
    let (status, _) = s.http("GET", "/fonts/css", &[("Sec-Fetch-Site", "same-origin")], "").await;
    assert_eq!(status, 400);
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
async fn duplicating_a_page_copies_its_artboards_in_one_undo_step() {
    let s = TestServer::start().await;
    let mut ws = connect(&s, &s.token).await.unwrap();
    let mut seen = Vec::new();
    let id =
        request(&mut ws, 1, json!({"t": "create", "name": "Dup"}), &mut seen).await["id"].as_str().unwrap().to_string();
    let snap = request(&mut ws, 2, json!({"t": "open", "ref": id}), &mut seen).await;
    let page = snap["page"].as_str().unwrap().to_string();
    request(
        &mut ws,
        3,
        json!({"t": "do", "file": id, "cmd": {"type": "setPage", "id": page, "prop": "icon", "value": "check"}}),
        &mut seen,
    )
    .await;
    seen.clear();
    let r =
        request(&mut ws, 4, json!({"t": "do", "file": id, "cmd": {"type": "duplicatePage", "id": page}}), &mut seen)
            .await;
    let copy = r["id"].as_str().unwrap();
    let patch = seen.iter().find(|m| m["t"] == "patch").unwrap();
    let pages = patch["pages"].as_array().unwrap();
    assert_eq!(pages.len(), 2);
    assert_eq!(pages[1]["id"], copy);
    assert_eq!(pages[1]["name"], "Page 1 copy");
    assert_eq!(pages[1]["icon"], "check");
    assert_eq!(patch["nodes"].as_object().unwrap().values().filter(|n| n["parent"] == copy).count(), 1);

    seen.clear();
    request(&mut ws, 5, json!({"t": "do", "file": id, "cmd": {"type": "undo"}}), &mut seen).await;
    let patch = seen.iter().find(|m| m["t"] == "patch").unwrap();
    assert_eq!(patch["pages"].as_array().unwrap().len(), 1, "one undo removes the page and its artboards");
}

#[tokio::test]
async fn duplicating_a_file_opens_an_independent_copy() {
    let s = TestServer::start().await;
    let mut ws = connect(&s, &s.token).await.unwrap();
    let mut seen = Vec::new();
    let id = request(&mut ws, 1, json!({"t": "create", "name": "Orig"}), &mut seen).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    request(&mut ws, 2, json!({"t": "open", "ref": id}), &mut seen).await;
    let copy = request(&mut ws, 3, json!({"t": "duplicate", "id": id}), &mut seen).await;
    assert_ne!(copy["id"], id.as_str());
    assert_eq!(copy["name"], "Orig copy");
    assert!(s.files_dir().join("Orig copy.spark").exists());
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
async fn pixel_snap_rounds_drag_and_resize_to_whole_pixels() {
    let s = TestServer::start().await;
    let mut ws = connect(&s, &s.token).await.unwrap();
    let mut seen = Vec::new();
    let id = request(&mut ws, 1, json!({"t": "create", "name": "Pixels"}), &mut seen).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let snap = request(&mut ws, 2, json!({"t": "open", "ref": id}), &mut seen).await;
    let board = snap["nodes"].as_object().unwrap().keys().next().unwrap().clone();
    let mk = json!({"type": "create", "kind": "rect", "parent": board, "x": 0, "y": 0, "w": 100, "h": 100});
    let a = request(&mut ws, 3, json!({"t": "do", "file": id, "cmd": mk}), &mut seen).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let rects =
        json!({"t": "rects", "file": id, "rects": {board.as_str(): [0, 0, 1440, 1024], a.as_str(): [0, 0, 100, 100]}});
    ws.send(Message::Text(rects.to_string().into())).await.unwrap();
    for (pixel, left, top) in [(true, "12px", "8px"), (false, "12.4px", "7.6px")] {
        request(&mut ws, 100, json!({"t": "do", "file": id, "cmd": {"type": "dragStart", "ids": [a]}}), &mut seen)
            .await;
        seen.clear();
        let cmd = json!({"type": "drag", "dx": 12.4, "dy": 7.6, "snap": false, "pixel": pixel, "zoom": 1});
        request(&mut ws, 103, json!({"t": "do", "file": id, "cmd": cmd}), &mut seen).await;
        let patch = seen.iter().find(|m| m["t"] == "patch").unwrap();
        assert_eq!(patch["nodes"][a.as_str()]["css"]["left"], left);
        assert_eq!(patch["nodes"][a.as_str()]["css"]["top"], top);
        request(&mut ws, 101, json!({"t": "do", "file": id, "cmd": {"type": "dragEnd"}}), &mut seen).await;
        request(&mut ws, 102, json!({"t": "do", "file": id, "cmd": {"type": "undo"}}), &mut seen).await;
    }
    request(&mut ws, 110, json!({"t": "do", "file": id, "cmd": {"type": "resizeStart", "ids": [a]}}), &mut seen).await;
    seen.clear();
    let rect = json!({"x": 0, "y": 0, "w": 120.6, "h": 99.4});
    request(
        &mut ws,
        111,
        json!({"t": "do", "file": id, "cmd": {"type": "resize", "rect": rect, "snap": false, "pixel": true}}),
        &mut seen,
    )
    .await;
    let patch = seen.iter().find(|m| m["t"] == "patch").unwrap();
    assert_eq!(patch["nodes"][a.as_str()]["css"]["width"], "121px");
    assert_eq!(patch["nodes"][a.as_str()]["css"]["height"], "99px");
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

#[tokio::test]
async fn style_text_replace_and_svg_intents() {
    let s = TestServer::start().await;
    let mut ws = connect(&s, &s.token).await.unwrap();
    let mut seen = Vec::new();
    let file = request(&mut ws, 1, json!({"t": "create", "name": "Clip"}), &mut seen).await;
    let id = file["id"].as_str().unwrap().to_string();
    let snap = request(&mut ws, 2, json!({"t": "open", "ref": id}), &mut seen).await;
    let board = snap["nodes"].as_object().unwrap().keys().next().unwrap().clone();
    let mut rid = 10;
    let mut go = async |ws: &mut Ws, seen: &mut Vec<Value>, cmd: Value| {
        rid += 1;
        request(ws, rid, json!({"t": "do", "file": id, "cmd": cmd}), seen).await
    };
    let a = go(
        &mut ws,
        &mut seen,
        json!({"type": "create", "kind": "rect", "parent": board, "x": 10, "y": 10, "w": 100, "h": 50}),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let b = go(
        &mut ws,
        &mut seen,
        json!({"type": "create", "kind": "rect", "parent": board, "x": 200, "y": 10, "w": 100, "h": 50}),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let t = go(
        &mut ws,
        &mut seen,
        json!({"type": "create", "kind": "text", "parent": board, "x": 10, "y": 100, "w": 0, "h": 0, "text": "Hi"}),
    )
    .await["id"]
        .as_str()
        .unwrap()
        .to_string();
    ws.send(Message::Text(json!({"t": "rects", "file": id, "rects": {&board: [0, 0, 1440, 1024], &a: [10, 10, 100, 50], &b: [200, 10, 100, 50]}}).to_string().into()))
        .await
        .unwrap();
    let node = |seen: &[Value], n: &str| seen.iter().rev().find_map(|m| m["nodes"].get(n).cloned()).unwrap();

    // Pasting a style sets appearance fields in one undo step; the size stays.
    let red = json!([{"type": "solid", "color": "#ff0000"}]);
    go(&mut ws, &mut seen, json!({"type": "pasteStyle", "ids": [b], "style": {"fills": red, "radius": ["8px", "8px", "8px", "8px"], "width": "5px"}})).await;
    let v = node(&seen, &b);
    assert!(v["style"]["fills"][0]["color"].as_str().unwrap().starts_with("oklch"));
    assert_eq!(v["style"]["radius"], json!(["8px", "8px", "8px", "8px"]));
    assert_eq!(v["style"]["width"], "100px");
    go(&mut ws, &mut seen, json!({"type": "undo"})).await;
    let v = node(&seen, &b);
    assert_eq!(v["style"]["fills"][0]["color"], "oklch(0.8853 0 0)");
    assert!(v["style"]["radius"].is_null());

    // Type fields only reach text layers.
    go(&mut ws, &mut seen, json!({"type": "pasteStyle", "ids": [a, t], "text": true, "style": {"fontSize": "30px"}}))
        .await;
    assert_eq!(node(&seen, &t)["style"]["fontSize"], "30px");
    assert!(node(&seen, &a)["style"]["fontSize"].is_null());

    // Adjust text steps each text layer by its own value.
    go(&mut ws, &mut seen, json!({"type": "adjustText", "ids": [t, a], "prop": "fontSize", "dir": -1})).await;
    assert_eq!(node(&seen, &t)["style"]["fontSize"], "29px");
    go(&mut ws, &mut seen, json!({"type": "adjustText", "ids": [t], "prop": "fontWeight", "dir": 1})).await;
    assert_eq!(node(&seen, &t)["style"]["fontWeight"], 500);

    // Replace swaps the target for the clipboard layers at its place, in one step.
    let mut clip = node(&seen, &a);
    for k in ["parent", "key", "css"] {
        clip.as_object_mut().unwrap().remove(k);
    }
    let r = go(&mut ws, &mut seen, json!({"type": "pasteSpark", "roots": [{"node": clip, "rect": {"x": 10, "y": 10, "w": 100, "h": 50}, "children": []}], "replace": [b]})).await;
    let new = r["ids"][0].as_str().unwrap().to_string();
    assert!(node(&seen, &b).is_null());
    assert_eq!(node(&seen, &new)["css"]["left"], "200px");
    go(&mut ws, &mut seen, json!({"type": "undo"})).await;
    assert!(node(&seen, &new).is_null());
    assert!(!node(&seen, &b).is_null());

    // SVG of a layer.
    let svg = go(&mut ws, &mut seen, json!({"type": "code", "ids": [a], "format": "svg"})).await;
    assert!(svg.as_str().unwrap().starts_with("<svg"), "{svg}");
}

#[tokio::test]
async fn replace_color_edits_a_selection_and_its_children_in_one_undo_step() {
    let s = TestServer::start().await;
    let mut ws = connect(&s, &s.token).await.unwrap();
    let mut seen = Vec::new();
    let id = request(&mut ws, 1, json!({"t": "create", "name": "Colors"}), &mut seen).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let snap = request(&mut ws, 2, json!({"t": "open", "ref": id}), &mut seen).await;
    let board = snap["nodes"].as_object().unwrap().keys().next().unwrap().clone();
    let mk = |x: i32| json!({"type": "create", "kind": "rect", "parent": board, "x": x, "y": 0, "w": 10, "h": 10});
    let a = request(&mut ws, 3, json!({"t": "do", "file": id, "cmd": mk(0)}), &mut seen).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let b = request(&mut ws, 4, json!({"t": "do", "file": id, "cmd": mk(20)}), &mut seen).await["id"]
        .as_str()
        .unwrap()
        .to_string();
    let styles = json!({"backgroundColor": "#ff0000", "border": "2px solid #ff0000"});
    request(
        &mut ws,
        5,
        json!({"t": "do", "file": id, "cmd": {"type": "setStyle", "ids": [a, b], "styles": styles}}),
        &mut seen,
    )
    .await;
    seen.clear();
    request(&mut ws, 6, json!({"t": "do", "file": id, "cmd": {"type": "replaceColor", "ids": [board], "from": "#ff0000", "to": "#00ff00"}}), &mut seen).await;
    let patch = seen.iter().find(|m| m["t"] == "patch").unwrap();
    for n in [&a, &b] {
        assert_eq!(patch["nodes"][n.as_str()]["css"]["backgroundColor"], "#00ff00");
        assert_eq!(patch["nodes"][n.as_str()]["css"]["border"], "2px solid #00ff00");
    }
    seen.clear();
    request(&mut ws, 7, json!({"t": "do", "file": id, "cmd": {"type": "undo"}}), &mut seen).await;
    let patch = seen.iter().find(|m| m["t"] == "patch").unwrap();
    assert_eq!(patch["nodes"][a.as_str()]["css"]["backgroundColor"], "#ff0000");
    assert_eq!(patch["nodes"][b.as_str()]["css"]["backgroundColor"], "#ff0000");
}

#[tokio::test]
async fn folders_move_duplicate_and_rename_files() {
    let s = TestServer::start().await;
    let mut ws = connect(&s, &s.token).await.unwrap();
    let mut seen = Vec::new();
    let file = request(&mut ws, 1, json!({"t": "create", "name": "Logo"}), &mut seen).await;
    let id = file["id"].as_str().unwrap().to_string();
    request(&mut ws, 2, json!({"t": "mkdir", "dir": "", "name": "Brand"}), &mut seen).await;

    let top = request(&mut ws, 3, json!({"t": "browse", "dir": ""}), &mut seen).await;
    assert_eq!(top["folders"], json!(["Brand"]));
    assert_eq!(top["files"][0]["folder"], "");

    // A file open in a tab can't move; closed, it does, and the list says where it is.
    request(&mut ws, 4, json!({"t": "open", "ref": id}), &mut seen).await;
    let busy = json!({"t": "move", "id": id, "dir": "Brand", "rid": 5});
    ws.send(Message::Text(busy.to_string().into())).await.unwrap();
    loop {
        let v: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        if v["t"] == "reply" && v["rid"] == json!(5) {
            assert_eq!(v["ok"], json!(false));
            break;
        }
    }
    request(&mut ws, 6, json!({"t": "close", "file": id}), &mut seen).await;
    let moved = request(&mut ws, 7, json!({"t": "move", "id": id, "dir": "Brand"}), &mut seen).await;
    assert!(moved["path"].as_str().unwrap().ends_with("Brand/Logo.spark"));
    assert!(s.files_dir().join("Brand/Logo.spark").exists());
    assert!(!s.files_dir().join("Logo.spark").exists());
    let inside = request(&mut ws, 8, json!({"t": "browse", "dir": "Brand"}), &mut seen).await;
    assert_eq!(inside["files"][0]["folder"], "Brand");

    // Escapes are refused and nothing moves.
    for dir in ["..", "../..", "Brand/../.."] {
        let bad = json!({"t": "move", "id": id, "dir": dir, "rid": 9});
        ws.send(Message::Text(bad.to_string().into())).await.unwrap();
        loop {
            let v: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            if v["t"] == "reply" && v["rid"] == json!(9) {
                assert_eq!(v["ok"], json!(false), "{dir}");
                break;
            }
        }
    }
    assert!(s.files_dir().join("Brand/Logo.spark").exists());

    // Duplicate lands beside the original under a new id; rename changes the listed name.
    let copy = request(&mut ws, 10, json!({"t": "duplicate", "id": id}), &mut seen).await;
    assert_ne!(copy["id"], json!(id));
    assert_eq!(copy["name"], "Logo copy");
    assert!(s.files_dir().join("Brand/Logo copy.spark").exists());
    request(&mut ws, 11, json!({"t": "rename", "id": id, "name": "Wordmark"}), &mut seen).await;
    // Renaming from an open tab (the `do` intent) reaches the list as well.
    request(&mut ws, 13, json!({"t": "open", "ref": copy["id"]}), &mut seen).await;
    request(
        &mut ws,
        14,
        json!({"t": "do", "file": copy["id"], "cmd": {"type": "renameFile", "name": "Tab rename"}}),
        &mut seen,
    )
    .await;
    let list = request(&mut ws, 12, json!({"t": "browse", "dir": "Brand"}), &mut seen).await;
    let names: Vec<&str> = list["files"].as_array().unwrap().iter().map(|f| f["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"Wordmark") && names.contains(&"Tab rename"), "{names:?}");
}

#[tokio::test]
async fn arrange_commands_align_distribute_ungroup_and_swap_colors() {
    let s = TestServer::start().await;
    let mut ws = connect(&s, &s.token).await.unwrap();
    let mut seen = Vec::new();
    let file = request(&mut ws, 1, json!({"t": "create", "name": "Arrange"}), &mut seen).await;
    let id = file["id"].as_str().unwrap().to_string();
    let snap = request(&mut ws, 2, json!({"t": "open", "ref": id}), &mut seen).await;
    let board = snap["nodes"].as_object().unwrap().keys().next().unwrap().clone();
    let mut rects = json!({ &board: [0, 0, 1440, 1024] });
    let mut made = Vec::new();
    for (i, (x, w)) in [(10, 50), (100, 80), (300, 40)].into_iter().enumerate() {
        let c = json!({"type": "create", "kind": "rect", "parent": board, "x": x, "y": 20 * i, "w": w, "h": 30});
        let r = request(&mut ws, 10 + i as u64, json!({"t": "do", "file": id, "cmd": c}), &mut seen).await;
        let rid = r["id"].as_str().unwrap().to_string();
        rects[&rid] = json!([x, 20 * i, w, 30]);
        made.push(rid);
    }
    ws.send(Message::Text(json!({"t": "rects", "file": id, "rects": rects}).to_string().into())).await.unwrap();
    let left = |seen: &[Value], n: &str| {
        seen.iter().rev().find_map(|m| m["nodes"][n]["css"]["left"].as_str().map(str::to_string))
    };
    let mut rid = 20;
    let mut run = async |ws: &mut Ws, seen: &mut Vec<Value>, cmd: Value| {
        rid += 1;
        request(ws, rid, json!({"t": "do", "file": id, "cmd": cmd}), seen).await
    };

    // Distribute: the middle layer lands so both gaps are equal (gap = (330 - 170) / 2 = 80).
    run(&mut ws, &mut seen, json!({"type": "distribute", "ids": made, "dir": "x"})).await;
    assert_eq!(left(&seen, &made[1]).as_deref(), Some("140px"));
    // Align right: every layer's right edge meets the union's (340).
    run(&mut ws, &mut seen, json!({"type": "align", "ids": made, "dir": "right"})).await;
    assert_eq!(left(&seen, &made[0]).as_deref(), Some("290px"));
    assert_eq!(left(&seen, &made[2]).as_deref(), Some("300px"));
    // Quarter turn and mirror.
    run(&mut ws, &mut seen, json!({"type": "transform", "ids": [made[0]], "dir": "rotate90"})).await;
    run(&mut ws, &mut seen, json!({"type": "transform", "ids": [made[0]], "dir": "flipH"})).await;
    let patch = seen.iter().rev().find(|m| m["nodes"][&made[0]].is_object()).unwrap();
    assert_eq!(patch["nodes"][&made[0]]["css"]["scale"], "-1 1");

    // Group two layers in a frame, then ungroup: they return to the board where they were.
    let w = run(&mut ws, &mut seen, json!({"type": "wrap", "ids": [made[0], made[1]]})).await;
    let frame = w["id"].as_str().unwrap().to_string();
    let r = run(&mut ws, &mut seen, json!({"type": "ungroup", "ids": [frame]})).await;
    assert_eq!(r["ids"].as_array().unwrap().len(), 2);
    let patch = seen.iter().rev().find(|m| m["nodes"][&made[1]]["parent"].is_string()).unwrap();
    assert_eq!(patch["nodes"][&made[1]]["parent"], board.as_str());
    assert_eq!(patch["nodes"][&frame], Value::Null);

    // Swap fill and border colours: red fill, blue border become blue fill, red border.
    run(&mut ws, &mut seen, json!({"type": "setStyle", "ids": [made[2]], "styles": {"background-color": "#ff0000", "border": "2px solid #0000ff"}})).await;
    run(&mut ws, &mut seen, json!({"type": "swapColors", "ids": [made[2]]})).await;
    let patch = seen.iter().rev().find(|m| m["nodes"][&made[2]]["style"]["border"].is_object()).unwrap();
    // Colours serialize as OKLCH; red is hue ~29.
    assert!(patch["nodes"][&made[2]]["style"]["border"]["color"].as_str().unwrap().starts_with("oklch(0.628"));
}

/// A file with one 50x50 rect at (100, 100), measured. Returns (file, rect).
async fn file_with_rect(ws: &mut Ws, seen: &mut Vec<Value>) -> (String, String) {
    let id = request(ws, 1, json!({"t": "create", "name": "Gesture"}), seen).await["id"].as_str().unwrap().to_string();
    let snap = request(ws, 2, json!({"t": "open", "ref": id}), seen).await;
    let board = snap["nodes"].as_object().unwrap().keys().next().unwrap().clone();
    let mk = json!({"type": "create", "kind": "rect", "parent": board, "x": 100, "y": 100, "w": 50, "h": 50});
    let a = request(ws, 3, json!({"t": "do", "file": id, "cmd": mk}), seen).await["id"].as_str().unwrap().to_string();
    let rects = json!({"t": "rects", "file": id, "rects": {board.as_str(): [0, 0, 1440, 1024], a.as_str(): [100, 100, 50, 50]}});
    ws.send(Message::Text(rects.to_string().into())).await.unwrap();
    (id, a)
}

#[tokio::test]
async fn a_window_that_goes_away_mid_drag_takes_its_drag_with_it() {
    let s = TestServer::start().await;
    let mut ws = connect(&s, &s.token).await.unwrap();
    let mut seen = Vec::new();
    let (id, a) = file_with_rect(&mut ws, &mut seen).await;
    let cmd = |c: Value| json!({"t": "do", "file": id, "cmd": c});
    request(&mut ws, 4, cmd(json!({"type": "dragStart", "ids": [a]})), &mut seen).await;
    request(&mut ws, 5, cmd(json!({"type": "drag", "dx": 40, "dy": 0, "snap": false})), &mut seen).await;
    drop(ws); // no dragEnd
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let mut ws = connect(&s, &s.token).await.unwrap();
    let snap = request(&mut ws, 6, json!({"t": "open", "ref": id}), &mut seen).await;
    assert_eq!(snap["nodes"][a.as_str()]["css"]["left"], "100px", "the unfinished drag is reverted");
    // Later edits are their own undo steps again.
    request(&mut ws, 7, cmd(json!({"type": "setStyle", "ids": [a], "styles": {"opacity": 0.5}})), &mut seen).await;
    request(&mut ws, 8, cmd(json!({"type": "set", "ids": [a], "prop": "name", "value": "Renamed"})), &mut seen).await;
    request(&mut ws, 9, cmd(json!({"type": "undo"})), &mut seen).await;
    let node = request(&mut ws, 10, json!({"t": "open", "ref": id}), &mut seen).await["nodes"][a.as_str()].clone();
    assert_eq!((&node["name"], &node["css"]["opacity"]), (&json!("Rectangle"), &json!("0.5")));
}

#[tokio::test]
async fn undo_during_a_drag_ends_the_drag() {
    let s = TestServer::start().await;
    let mut ws = connect(&s, &s.token).await.unwrap();
    let mut seen = Vec::new();
    let (id, a) = file_with_rect(&mut ws, &mut seen).await;
    let cmd = |c: Value| json!({"t": "do", "file": id, "cmd": c});
    request(&mut ws, 4, cmd(json!({"type": "dragStart", "ids": [a]})), &mut seen).await;
    request(&mut ws, 5, cmd(json!({"type": "drag", "dx": 40, "dy": 0, "snap": false})), &mut seen).await;
    request(&mut ws, 6, cmd(json!({"type": "undo"})), &mut seen).await;
    // Frames still in flight are refused instead of each becoming an undo step.
    let mut frame = cmd(json!({"type": "drag", "dx": 60, "dy": 0, "snap": false}));
    frame["rid"] = json!(7);
    ws.send(Message::Text(frame.to_string().into())).await.unwrap();
    let reply = loop {
        let v: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        if v["rid"] == json!(7) {
            break v;
        }
    };
    assert_eq!(reply["error"], "no drag in progress");
    let snap = request(&mut ws, 8, json!({"t": "open", "ref": id}), &mut seen).await;
    assert_eq!(snap["nodes"][a.as_str()]["css"]["left"], "100px");
    assert_eq!(snap["canUndo"], true, "creating the layer is still on the history");
}

#[tokio::test]
async fn undo_during_a_rotate_drops_its_late_frames() {
    let s = TestServer::start().await;
    let mut ws = connect(&s, &s.token).await.unwrap();
    let mut seen = Vec::new();
    let (id, a) = file_with_rect(&mut ws, &mut seen).await;
    let cmd = |c: Value| json!({"t": "do", "file": id, "cmd": c});
    let frame =
        |deg: i32| json!({"type": "setStyle", "gesture": true, "ids": [a], "styles": {"rotate": format!("{deg}deg")}});
    request(&mut ws, 4, cmd(json!({"type": "begin"})), &mut seen).await;
    request(&mut ws, 5, cmd(frame(10)), &mut seen).await;
    request(&mut ws, 6, cmd(json!({"type": "undo"})), &mut seen).await;
    for (rid, deg) in (7..17).zip(20..) {
        let mut m = cmd(frame(deg));
        m["rid"] = json!(rid);
        ws.send(Message::Text(m.to_string().into())).await.unwrap();
        let reply = loop {
            let v: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            if v["rid"] == json!(rid) {
                break v;
            }
        };
        assert_eq!(reply["error"], "no gesture in progress");
    }
    let snap = request(&mut ws, 20, json!({"t": "open", "ref": id}), &mut seen).await;
    assert!(snap["nodes"][a.as_str()]["css"]["rotate"].is_null(), "{}", snap["nodes"][a.as_str()]);
}
