//! MCP over Streamable HTTP at `/mcp`. Requests are JSON-RPC over POST; replies are plain JSON.
//! Sessions are created on demand from any POST, so agents survive a Spark restart.

mod calls;
pub mod tools;

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock},
};

use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

use crate::AppState;

pub const PROTOCOL_VERSIONS: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const SESSION_HEADER: &str = "mcp-session-id";

#[derive(Clone, Default)]
struct Session {
    name: String,
    transport: String,
}

fn sessions() -> &'static Mutex<HashMap<String, Session>> {
    static S: OnceLock<Mutex<HashMap<String, Session>>> = OnceLock::new();
    S.get_or_init(Mutex::default)
}

fn new_session_id() -> String {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("os random");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Friendly agent names for common MCP clients.
pub fn agent_name(info: &Value) -> String {
    let raw = info["title"].as_str().or(info["name"].as_str()).unwrap_or("Agent");
    match raw.to_ascii_lowercase().as_str() {
        "claude-code" | "claude code" => "Claude Code".into(),
        "codex" | "codex-mcp-client" | "codex_cli" | "codex-cli" => "Codex".into(),
        "cursor" | "cursor-vscode" => "Cursor".into(),
        "visual studio code" | "vscode" => "VS Code".into(),
        "opencode" => "OpenCode".into(),
        "gemini-cli-mcp-client" | "gemini-cli" => "Gemini CLI".into(),
        "claude-ai" | "claude desktop" => "Claude".into(),
        _ => raw.to_string(),
    }
}

fn unauthorized() -> Response {
    // Plain text on purpose: OAuth-shaped JSON makes some clients start an OAuth flow.
    (
        StatusCode::UNAUTHORIZED,
        "Spark rejected the request: missing or wrong bearer token. Reconnect the agent with `spark connect`.",
    )
        .into_response()
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// The tool list and instructions, without a session (used by the stdio relay).
pub fn config_json() -> Value {
    json!({"tools": tools::catalog(), "instructions": tools::INSTRUCTIONS, "serverInfo": {"name": "spark", "version": spark_core::VERSION}})
}

pub async fn config(State(app): State<AppState>, headers: HeaderMap) -> Response {
    if !app.token_ok(&headers, None) {
        return unauthorized();
    }
    axum::Json(config_json()).into_response()
}

pub async fn get() -> Response {
    // No server-initiated stream: every reply travels on its POST.
    (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST, DELETE")]).into_response()
}

pub async fn delete(State(app): State<AppState>, headers: HeaderMap) -> Response {
    if !app.token_ok(&headers, None) {
        return unauthorized();
    }
    if let Some(id) = headers.get(SESSION_HEADER).and_then(|v| v.to_str().ok()) {
        sessions().lock().unwrap().remove(id);
        app.hub.remove_agent(id);
    }
    StatusCode::OK.into_response()
}

pub async fn post(State(app): State<AppState>, headers: HeaderMap, body: Bytes) -> Response {
    if !app.token_ok(&headers, None) {
        return unauthorized();
    }
    let session = headers
        .get(SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty() && s.len() <= 128)
        .map(str::to_string)
        .unwrap_or_else(new_session_id);
    let transport = if headers.get("x-spark-from").is_some_and(|v| v == "cli") { "stdio" } else { "http" };
    sessions()
        .lock()
        .unwrap()
        .entry(session.clone())
        .or_insert_with(|| Session { name: String::new(), transport: transport.into() });

    let parsed: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, axum::Json(rpc_error(Value::Null, -32700, &format!("parse error: {e}"))))
                .into_response();
        }
    };
    let batch = parsed.is_array();
    let messages = if let Value::Array(a) = parsed { a } else { vec![parsed] };
    let mut replies = Vec::new();
    for msg in messages {
        if let Some(reply) = handle(&app, &session, msg).await {
            replies.push(reply);
        }
    }
    let mut res = if replies.is_empty() {
        StatusCode::ACCEPTED.into_response()
    } else if batch {
        axum::Json(Value::Array(replies)).into_response()
    } else {
        axum::Json(replies.into_iter().next().unwrap()).into_response()
    };
    if let Ok(v) = HeaderValue::from_str(&session) {
        res.headers_mut().insert(SESSION_HEADER, v);
    }
    res
}

/// Handles one JSON-RPC message. Returns None for notifications and responses.
async fn handle(app: &AppState, session: &str, msg: Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg["method"].as_str()?;
    let params = &msg["params"];
    // Client identity rides on initialize, or on any request's _meta (the stdio relay adds it).
    let info = if method == "initialize" {
        params["clientInfo"].clone()
    } else {
        params["_meta"]["io.modelcontextprotocol/clientInfo"].clone()
    };
    if info.is_object()
        && let Some(s) = sessions().lock().unwrap().get_mut(session)
    {
        s.name = agent_name(&info);
    }
    let id = id?;
    let result = match method {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or(PROTOCOL_VERSIONS[0]);
            let version = if PROTOCOL_VERSIONS.contains(&asked) { asked } else { PROTOCOL_VERSIONS[0] };
            Ok(json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": "spark", "title": "Spark", "version": spark_core::VERSION},
                "instructions": tools::INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tools::catalog()})),
        "tools/call" => {
            let name = params["name"].as_str().unwrap_or_default();
            if !tools::catalog().iter().any(|t| t["name"] == name) {
                return Some(rpc_error(id, -32602, &format!("unknown tool `{name}`")));
            }
            let s = sessions().lock().unwrap().get(session).cloned().unwrap_or_default();
            app.hub.agent_seen(session, if s.name.is_empty() { "Agent" } else { &s.name }, &s.transport);
            let args = if params["arguments"].is_object() { params["arguments"].clone() } else { json!({}) };
            let started = std::time::Instant::now();
            let result = calls::call(app, session, name, &args).await;
            app.hub.broadcast_all(&json!({
                "t": "toolCall", "agent": if s.name.is_empty() { "Agent" } else { &s.name }, "session": session,
                "tool": name, "ms": started.elapsed().as_millis() as u64, "ok": result.is_ok(),
                "error": result.as_ref().err(),
            }));
            Ok(match result {
                Ok(content) => json!({"content": content}),
                Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
            })
        }
        "resources/list" => Ok(json!({"resources": []})),
        "prompts/list" => Ok(json!({"prompts": []})),
        other => Err(rpc_error(id.clone(), -32601, &format!("method `{other}` not supported"))),
    };
    Some(match result {
        Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
        Err(e) => e,
    })
}
