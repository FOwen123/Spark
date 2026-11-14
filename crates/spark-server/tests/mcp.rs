mod common;

use common::TestServer;
use serde_json::{Value, json};

impl TestServer {
    async fn rpc(&self, session: &str, method: &str, params: Value) -> (u16, Value) {
        let body = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string();
        let auth = format!("Bearer {}", self.token);
        let mut headers = vec![
            ("Authorization", auth.as_str()),
            ("Content-Type", "application/json"),
            ("Accept", "application/json, text/event-stream"),
        ];
        if !session.is_empty() {
            headers.push(("mcp-session-id", session));
        }
        let (status, body) = self.http("POST", "/mcp", &headers, &body).await;
        (status, serde_json::from_str(&body).unwrap_or(Value::Null))
    }

    /// Calls a tool and returns (isError, content).
    async fn tool(&self, name: &str, args: Value) -> (bool, Vec<Value>) {
        let (status, v) = self.rpc("s1", "tools/call", json!({"name": name, "arguments": args})).await;
        assert_eq!(status, 200, "{v}");
        (
            v["result"]["isError"].as_bool().unwrap_or(false),
            v["result"]["content"].as_array().cloned().unwrap_or_default(),
        )
    }

    async fn tool_json(&self, name: &str, args: Value) -> Value {
        let (err, content) = self.tool(name, args).await;
        assert!(!err, "{name} failed: {content:?}");
        serde_json::from_str(content.last().unwrap()["text"].as_str().unwrap()).unwrap()
    }
}

#[tokio::test]
async fn protocol_basics_and_auth() {
    let s = TestServer::start().await;
    let (status, _) = s.http("POST", "/mcp", &[("Content-Type", "application/json")], "{}").await;
    assert_eq!(status, 401);
    let (status, _) = s
        .http(
            "POST",
            "/mcp",
            &[("Authorization", &format!("Bearer {}", s.token)), ("Origin", "http://evil.example")],
            "{}",
        )
        .await;
    assert_eq!(status, 403);

    let (status, v) = s.rpc("", "initialize", json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "claude-code", "version": "1"}})).await;
    assert_eq!(status, 200);
    assert_eq!(v["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(v["result"]["serverInfo"]["name"], "spark");
    assert!(v["result"]["instructions"].as_str().unwrap().contains("spark-mcp-instructions"));
    let (_, v) = s.rpc("", "initialize", json!({"protocolVersion": "1999-01-01"})).await;
    assert_eq!(v["result"]["protocolVersion"], "2025-11-25");

    let note = json!({"jsonrpc": "2.0", "method": "notifications/initialized"}).to_string();
    let (status, _) = s.http("POST", "/mcp", &[("Authorization", &format!("Bearer {}", s.token))], &note).await;
    assert_eq!(status, 202);

    let (_, v) = s.rpc("x", "tools/list", json!({})).await;
    let tools = v["result"]["tools"].as_array().unwrap();
    let find = |n: &str| tools.iter().find(|t| t["name"] == n).unwrap().clone();
    assert_eq!(find("get_jsx")["annotations"]["readOnlyHint"], true);
    assert_eq!(find("write_html")["annotations"]["readOnlyHint"], false);
    assert_eq!(find("delete_nodes")["annotations"]["destructiveHint"], true);
    assert_eq!(find("finish_working_on_nodes")["annotations"]["readOnlyHint"], false);

    let (status, _) = s.http("GET", "/mcp", &[("Authorization", &format!("Bearer {}", s.token))], "").await;
    assert_eq!(status, 405);
    let (status, _) = s.http("DELETE", "/mcp", &[("Mcp-Session-Id", "x")], "").await;
    assert_eq!(status, 401);
    let (status, _) = s
        .http("DELETE", "/mcp", &[("Authorization", &format!("Bearer {}", s.token)), ("Mcp-Session-Id", "x")], "")
        .await;
    assert_eq!(status, 200);
    let (status, body) =
        s.http("GET", "/mcp/config.json", &[("Authorization", &format!("Bearer {}", s.token))], "").await;
    assert_eq!(status, 200);
    assert!(body.contains("write_html"));

    let (_, v) = s.rpc("x", "tools/call", json!({"name": "nope", "arguments": {}})).await;
    assert_eq!(v["error"]["code"], -32602);
    let (_, v) = s.rpc("x", "get_guide", json!({})).await;
    assert_eq!(v["error"]["code"], -32601);
    let (err, c) = s.tool("get_guide", json!({"topic": "spark-mcp-instructions"})).await;
    assert!(!err && c[0]["text"].as_str().unwrap().starts_with("# Working in Spark"));
}

#[tokio::test]
async fn builds_a_design_through_tools() {
    let s = TestServer::start().await;
    let (err, c) = s.tool("get_basic_info", json!({})).await;
    assert!(err, "no file open yet");
    assert!(c[0]["text"].as_str().unwrap().contains("No file is open"));

    let file = s.tool_json("create_file", json!({"name": "Agent work"})).await;
    let id = file["fileId"].as_str().unwrap().to_string();
    let info = s.tool_json("get_basic_info", json!({"fileId": id})).await;
    assert_eq!(info["fileName"], "Agent work");
    assert_eq!(info["artboardCount"], 1);

    // Header block names the file and hashes tokens (empty set = FNV offset basis).
    let (_, c) = s.tool("get_basic_info", json!({"fileId": format!("/file/{id}/p-1")})).await;
    let head: Value = serde_json::from_str(c[0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(head["file"]["id"], id.as_str());
    assert_eq!(head["contentHash"]["tokens"], "811c9dc5");

    let board = s
        .tool_json(
            "create_artboard",
            json!({"fileId": id, "name": "Landing", "styles": {"width": "800px", "height": "600px"}}),
        )
        .await;
    let board_id = board["id"].as_str().unwrap().to_string();
    let info = s.tool_json("get_basic_info", json!({"fileId": format!("spark://file/{id}")})).await;
    let placed = info["artboards"].as_array().unwrap().iter().find(|a| a["id"] == board_id.as_str()).unwrap().clone();
    assert_eq!(placed["name"], "Landing");

    let html = r##"<div layer-name="Hero" style="display:flex; flex-direction:column; gap:12px; padding:32px">
        <h1 style="font-size:48px; color:#111">Ship designs as code</h1>
        <p style="color:#555">Spark is real HTML and CSS.</p>
        <img src="spark-gen://agent?prompt=A%20calm%20workspace&aspect_ratio=16:9" style="width:100%">
    </div>"##;
    let w = s.tool_json("write_html", json!({"fileId": id, "targetNodeId": board_id, "html": html})).await;
    let hero = w["created"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(w["created"][0]["name"], "Hero");

    let tree = s.tool_json("get_tree_summary", json!({"fileId": id, "nodeId": board_id, "depth": 4})).await;
    let summary = tree["summary"].as_str().unwrap();
    assert!(summary.contains("Frame \"Hero\"") && summary.contains("Ship designs as code"), "{summary}");

    let (_, c) = s.tool("get_jsx", json!({"fileId": id, "nodeId": hero, "format": "tailwind"})).await;
    let jsx = c[1]["text"].as_str().unwrap();
    assert!(jsx.contains("flex flex-col gap-3 p-8") && jsx.contains("Ship designs as code"), "{jsx}");

    let found = s.tool_json("find_nodes", json!({"fileId": id, "textValue": "ship*code"})).await;
    assert_eq!(found["count"], 1);
    let found = s
        .tool_json("find_nodes", json!({"fileId": id, "filters": [{"styleName": "color", "styleValue": "#111111"}]}))
        .await;
    assert_eq!(found["count"], 1, "colors match by value");

    let text_id = found["nodes"][0]["id"].as_str().unwrap().to_string();
    s.tool_json(
        "update_styles",
        json!({"fileId": id, "updates": [{"nodeIds": [text_id], "styles": {"fontSize": 56, "bogus prop": "x"}}]}),
    )
    .await;
    let styles = s.tool_json("get_computed_styles", json!({"fileId": id, "nodeIds": [text_id]})).await;
    assert_eq!(styles["styles"][&text_id]["fontSize"], "56px");
    s.tool_json(
        "set_text_content",
        json!({"fileId": id, "updates": [{"nodeId": text_id, "textContent": "Design in code"}]}),
    )
    .await;
    s.tool_json("rename_nodes", json!({"fileId": id, "updates": [{"nodeId": text_id, "name": "Headline"}]})).await;
    let node = s.tool_json("get_node_info", json!({"fileId": id, "nodeId": text_id})).await;
    assert_eq!((node["textContent"].as_str(), node["name"].as_str()), (Some("Design in code"), Some("Headline")));

    // Image requests show on the placeholder.
    let img = s
        .tool_json(
            "find_nodes",
            json!({"fileId": id, "nodeId": hero, "filters": [{"styleName": "width", "styleValue": "100%"}]}),
        )
        .await;
    let img_id = img["nodes"][0]["id"].as_str().unwrap().to_string();
    let node = s.tool_json("get_node_info", json!({"fileId": id, "nodeId": img_id})).await;
    assert_eq!(node["imageGeneration"]["status"], "pending");

    // Structure tools.
    let dup = s.tool_json("duplicate_nodes", json!({"fileId": id, "nodes": [{"id": hero}]})).await;
    let copy = dup["nodes"][0]["newId"].as_str().unwrap().to_string();
    assert_eq!(dup["descendantIdMap"].as_object().unwrap().len(), 4);
    let moved = s.tool_json("move_nodes", json!({"fileId": id, "moves": [{"nodeId": copy, "before": hero}]})).await;
    assert_eq!(moved["moves"][0]["index"], 0);
    s.tool_json("delete_nodes", json!({"fileId": id, "nodeIds": [copy]})).await;
    let (err, _) = s.tool("get_node_info", json!({"fileId": id, "nodeId": copy})).await;
    assert!(err);

    // Tokens.
    let created = s.tool_json("create_tokens", json!({"fileId": id, "tokens": [{"name": "--color-ink", "value": "#111111"}, {"name": "--color-alias", "value": "var(--color-ink)"}, {"name": "--nope", "value": "1"}]})).await;
    assert_eq!(created[2]["result"], "error");
    let (_, c) = s.tool("get_tokens", json!({"fileId": id, "format": "tailwind"})).await;
    assert!(c[1]["text"].as_str().unwrap().contains("--color-alias: var(--color-ink);"));
    let toks = s.tool_json("get_tokens", json!({"fileId": id})).await;
    assert_eq!(toks["tokens"][0]["resolvedValue"], "#111111");
    let (_, c) = s.tool("get_basic_info", json!({"fileId": id})).await;
    let head: Value = serde_json::from_str(c[0]["text"].as_str().unwrap()).unwrap();
    assert_ne!(head["contentHash"]["tokens"], "811c9dc5", "token hash changes with tokens");
    let found =
        s.tool_json("find_nodes", json!({"fileId": id, "filters": [{"styleValue": "var(--color-alias)"}]})).await;
    assert_eq!(found["count"], 0);

    // Style guide.
    s.tool_json("set_style_guide", json!({"fileId": id, "text": "# Rules\n- 8px grid"})).await;
    let g = s.tool_json("set_style_guide", json!({"fileId": id, "decision": "No shadows"})).await;
    assert_eq!(g["styleGuide"], "# Rules\n- 8px grid\n\n## Decisions\n\n- No shadows\n");
    let info = s.tool_json("get_basic_info", json!({"fileId": id})).await;
    assert_eq!(info["styleGuide"], g["styleGuide"]);
    let (_, c) = s.tool("get_design_md", json!({"fileId": id})).await;
    let md = c[1]["text"].as_str().unwrap();
    assert!(md.starts_with("# Rules") && md.contains("- No shadows") && md.contains("--color-ink: #111111;"));
    let (err, _) = s.tool("set_style_guide", json!({"fileId": id})).await;
    assert!(err, "needs text or decision");

    // Pages.
    let p = s.tool_json("create_page", json!({"fileId": id, "name": "Mobile"})).await;
    let pid = p["pageId"].as_str().unwrap().to_string();
    let arranged =
        s.tool_json("arrange_pages", json!({"fileId": id, "order": [pid, {"type": "divider"}, "p-1"]})).await;
    assert_eq!(arranged["pages"][1]["type"], "divider");
    let (err, _) = s.tool("arrange_pages", json!({"fileId": id, "order": ["p-1"]})).await;
    assert!(err, "incomplete order is rejected");

    // Screenshots need a UI.
    let (err, c) = s.tool("get_screenshot", json!({"fileId": id, "nodeId": board_id})).await;
    assert!(err && c[0]["text"].as_str().unwrap().contains("open Spark"));

    // A missing target fails instead of writing elsewhere.
    let (err, _) = s.tool("get_basic_info", json!({"fileId": "01ZZZZZZZZZZZZZZZZZZZZZZZZ"})).await;
    assert!(err);
    let listed = s.tool_json("list_resources", json!({})).await;
    assert_eq!(listed["resources"][0]["id"], id.as_str());
}

#[tokio::test]
async fn each_tool_call_is_one_undo_step() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
    let s = TestServer::start().await;
    let file = s.tool_json("create_file", json!({"name": "Undo"})).await;
    let id = file["fileId"].as_str().unwrap().to_string();
    let info = s.tool_json("get_basic_info", json!({"fileId": id})).await;
    let board = info["artboards"][0]["id"].as_str().unwrap().to_string();
    s.tool_json(
        "write_html",
        json!({"fileId": id, "targetNodeId": board, "html": "<div><p>A</p><p>B</p><p>C</p></div>"}),
    )
    .await;
    let after = s.tool_json("get_basic_info", json!({"fileId": id})).await;
    assert_eq!(after["nodeCount"], 5);

    let req = format!("ws://127.0.0.1:{}/ui?token={}", s.port, s.token).into_client_request().unwrap();
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    ws.send(Message::Text(json!({"t": "open", "ref": id, "rid": 1}).to_string().into())).await.unwrap();
    ws.send(Message::Text(json!({"t": "do", "file": id, "rid": 2, "cmd": {"type": "undo"}}).to_string().into()))
        .await
        .unwrap();
    loop {
        let m: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
        if m["rid"] == 2 {
            break;
        }
    }
    let undone = s.tool_json("get_basic_info", json!({"fileId": id})).await;
    assert_eq!(undone["nodeCount"], 1, "the whole write_html undoes at once");
}

#[tokio::test]
async fn bad_tool_input_is_an_error_not_a_broken_file() {
    let s = TestServer::start().await;
    let file = s.tool_json("create_file", json!({"name": "Robust"})).await;
    let id = file["fileId"].as_str().unwrap().to_string();
    let info = s.tool_json("get_basic_info", json!({"fileId": id})).await;
    let board = info["artboards"][0]["id"].as_str().unwrap().to_string();
    let (err, _) = s.tool("move_nodes", json!({"fileId": id, "moves": [{"nodeId": board, "before": board}]})).await;
    assert!(err, "moving a layer before itself is rejected");
    s.tool(
        "write_html",
        json!({"fileId": id, "targetNodeId": board, "html": "<img src=\"spark-gen://agent?prompt=%💥\">"}),
    )
    .await;
    s.tool_json("get_basic_info", json!({"fileId": id})).await;
}

#[tokio::test]
async fn agent_calls_leave_an_open_ui_gesture_alone() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
    let s = TestServer::start().await;
    let file = s.tool_json("create_file", json!({"name": "Gesture"})).await;
    let id = file["fileId"].as_str().unwrap().to_string();
    let info = s.tool_json("get_basic_info", json!({"fileId": id})).await;
    let board = info["artboards"][0]["id"].as_str().unwrap().to_string();
    let req = format!("ws://127.0.0.1:{}/ui?token={}", s.port, s.token).into_client_request().unwrap();
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    let mut rid = 0;
    let mut send = async |msg: Value| {
        rid += 1;
        let mut msg = msg;
        msg["rid"] = json!(rid);
        ws.send(Message::Text(msg.to_string().into())).await.unwrap();
        loop {
            let m: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            if m["rid"] == rid {
                break;
            }
        }
    };
    send(json!({"t": "open", "ref": id})).await;
    send(json!({"t": "do", "file": id, "cmd": {"type": "begin"}})).await;
    send(
        json!({"t": "do", "file": id, "cmd": {"type": "set", "ids": [board], "prop": "name", "value": "Mid-gesture"}}),
    )
    .await;
    // A failing agent call rolls back only its own changes.
    let (err, _) = s.tool("write_html", json!({"fileId": id, "targetNodeId": "zzz-9", "html": "<p>x</p>"})).await;
    assert!(err);
    let node = s.tool_json("get_node_info", json!({"fileId": id, "nodeId": board})).await;
    assert_eq!(node["name"], "Mid-gesture", "the UI's unfinished edit survives");
    // The gesture is still open, so cancelling it reverts the UI edit.
    send(json!({"t": "do", "file": id, "cmd": {"type": "cancel"}})).await;
    let node = s.tool_json("get_node_info", json!({"fileId": id, "nodeId": board})).await;
    assert_ne!(node["name"], "Mid-gesture");
}

#[tokio::test]
async fn a_fulfilled_image_stays_ready_across_undo_and_redo() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
    const PNG: &str =
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
    let s = TestServer::start().await;
    let id = s.tool_json("create_file", json!({"name": "Image"})).await["fileId"].as_str().unwrap().to_string();
    let board = s.tool_json("get_basic_info", json!({"fileId": id})).await["artboards"][0]["id"].clone();
    let html = r#"<img src="spark-gen://agent?prompt=kite" style="width:100px">"#;
    let node = s.tool_json("write_html", json!({"fileId": id, "targetNodeId": board, "html": html})).await["created"]
        [0]["id"]
        .clone();
    let image =
        async || s.tool_json("get_node_info", json!({"fileId": id, "nodeId": node})).await["imageGeneration"].clone();
    let request = image().await["requestId"].clone();
    s.tool_json("fulfill_image_request", json!({"fileId": id, "requestId": request, "base64": PNG})).await;

    let req = format!("ws://127.0.0.1:{}/ui?token={}", s.port, s.token).into_client_request().unwrap();
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    for (rid, cmd) in ["undo", "redo"].into_iter().enumerate() {
        let msg = json!({"t": "do", "file": id, "rid": rid, "cmd": {"type": cmd}});
        ws.send(Message::Text(msg.to_string().into())).await.unwrap();
        while serde_json::from_str::<Value>(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap()["rid"] != rid
        {
        }
    }
    assert_eq!(image().await["status"], "ready");
    let (_, fill) = s.tool("get_fill_image", json!({"fileId": id, "nodeId": node})).await;
    assert!(fill.iter().any(|c| c["type"] == "image"), "the image itself is back too: {fill:?}");
}

#[tokio::test]
async fn opening_a_copy_of_an_open_file_opens_the_copy() {
    let s = TestServer::start().await;
    let file = s.tool_json("create_file", json!({"name": "Original"})).await;
    let id = file["fileId"].as_str().unwrap().to_string();
    let (_, content) = s.tool("get_basic_info", json!({"fileId": id})).await;
    let header: Value = serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap();
    let original = std::path::PathBuf::from(header["file"]["path"].as_str().unwrap());
    let copy = original.with_file_name("Copy.spark");
    for ext in ["", "-wal"] {
        let from = format!("{}{ext}", original.display());
        if std::path::Path::new(&from).exists() {
            std::fs::copy(&from, format!("{}{ext}", copy.display())).unwrap();
        }
    }
    let (_, content) = s.tool("open_file", json!({"fileId": copy.to_str().unwrap()})).await;
    let opened: Value = serde_json::from_str(content[0]["text"].as_str().unwrap()).unwrap();
    assert_ne!(opened["file"]["id"], json!(id), "the copy gets its own id");
    assert_eq!(opened["file"]["path"], json!(copy.canonicalize().unwrap()));
}

#[tokio::test]
async fn deep_nesting_is_refused_instead_of_crashing() {
    let s = TestServer::start().await;
    let file = s.tool_json("create_file", json!({"name": "Deep"})).await;
    let id = file["fileId"].as_str().unwrap().to_string();
    let info = s.tool_json("get_basic_info", json!({"fileId": id})).await;
    let board = info["artboards"][0]["id"].as_str().unwrap().to_string();
    let nest = |n: usize| "<div style=\"display:flex\">".repeat(n) + &"</div>".repeat(n);
    let (err, _) = s.tool("write_html", json!({"fileId": id, "targetNodeId": board, "html": nest(5000)})).await;
    assert!(err, "a design nested 5000 levels deep is refused");
    // The deepest design Spark allows still lays out, exports and duplicates.
    s.tool_json(
        "write_html",
        json!({"fileId": id, "targetNodeId": board, "html": nest(spark_core::doc::MAX_DEPTH - 1)}),
    )
    .await;
    let (err, _) = s.tool("get_jsx", json!({"fileId": id, "nodeId": board})).await;
    assert!(!err);
    let formats = json!([{"format": "svg"}, {"format": "png"}, {"format": "pdf"}]);
    let dir = s.files_dir().join("exports");
    s.tool_json("export", json!({"fileId": id, "nodes": {board.clone(): formats}, "dir": dir})).await;
    s.tool_json("duplicate_nodes", json!({"fileId": id, "nodes": [{"id": board}]})).await;
    let (err, _) = s.tool("write_html", json!({"fileId": id, "targetNodeId": board, "html": nest(3)})).await;
    assert!(!err, "shallow writes into the board still work");
}

#[tokio::test]
async fn stale_or_locked_targets_are_errors_not_a_stuck_file() {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::{Message, client::IntoClientRequest};
    let s = TestServer::start().await;
    let file = s.tool_json("create_file", json!({"name": "Stale"})).await;
    let id = file["fileId"].as_str().unwrap().to_string();
    let info = s.tool_json("get_basic_info", json!({"fileId": id})).await;
    let board = info["artboards"][0]["id"].as_str().unwrap().to_string();
    let req = format!("ws://127.0.0.1:{}/ui?token={}", s.port, s.token).into_client_request().unwrap();
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    let mut rid = 0;
    let mut send = async |msg: Value| {
        rid += 1;
        let mut msg = msg;
        msg["rid"] = json!(rid);
        ws.send(Message::Text(msg.to_string().into())).await.unwrap();
        loop {
            let m: Value = serde_json::from_str(ws.next().await.unwrap().unwrap().to_text().unwrap()).unwrap();
            if m["rid"] == rid {
                return m;
            }
        }
    };
    send(json!({"t": "open", "ref": id})).await;
    send(json!({"t": "do", "file": id, "cmd": {"type": "set", "ids": [board], "prop": "locked", "value": true}})).await;
    let (err, _) = s.tool("duplicate_nodes", json!({"fileId": id, "nodes": [{"id": board}]})).await;
    assert!(err, "duplicating a locked layer is an error");
    send(json!({"t": "do", "file": id, "cmd": {"type": "set", "ids": [board], "prop": "locked", "value": false}}))
        .await;
    // An agent deletes the layer the user is resizing.
    send(json!({"t": "do", "file": id, "cmd": {"type": "resizeStart", "ids": [board]}})).await;
    s.tool_json("delete_nodes", json!({"fileId": id, "nodeIds": [board]})).await;
    let rect = json!({"x": 0, "y": 0, "w": 500, "h": 500});
    send(json!({"t": "do", "file": id, "cmd": {"type": "resize", "rect": rect}})).await;
    send(json!({"t": "do", "file": id, "cmd": {"type": "resizeEnd"}})).await;
    s.tool_json("get_basic_info", json!({"fileId": id})).await;
}
