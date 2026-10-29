//! `spark mcp`: an MCP stdio server that relays to the running Spark over HTTP. It answers
//! `initialize` and `tools/list` itself, so agents always see Spark's tools, and turns "Spark is
//! closed" into a readable tool result instead of a transport error.

use std::{path::PathBuf, time::Duration};

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::{Config, mcp};

pub const OFFLINE: &str = "Spark isn't running, so this tool can't reach it. Ask the user to open Spark, then try again. Don't try to start Spark yourself.";

struct Relay {
    config: Config,
    client: reqwest::Client,
    session: Option<String>,
    client_info: Value,
    headless: bool,
    started: bool,
    web_dir: Option<PathBuf>,
}

impl Relay {
    fn base(&self) -> String {
        let port = std::fs::read_to_string(self.config.dir.join("port"))
            .ok()
            .and_then(|p| p.trim().parse::<u16>().ok())
            .unwrap_or(29890);
        format!("http://127.0.0.1:{port}")
    }

    async fn post(&mut self, msg: &Value) -> Result<Option<Value>, reqwest::Error> {
        let mut req = self
            .client
            .post(format!("{}/mcp", self.base()))
            .bearer_auth(&self.config.token)
            .header("x-spark-from", "cli")
            .header("accept", "application/json, text/event-stream")
            .json(msg);
        if let Some(s) = &self.session {
            req = req.header("mcp-session-id", s);
        }
        let res = req.send().await?;
        if let Some(s) = res.headers().get("mcp-session-id").and_then(|v| v.to_str().ok()) {
            self.session = Some(s.to_string());
        }
        if res.status() == reqwest::StatusCode::ACCEPTED {
            return Ok(None);
        }
        if res.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Ok(Some(
                json!({"jsonrpc": "2.0", "id": msg["id"], "result": {"isError": true, "content": [{"type": "text", "text": "Spark rejected the relay's token. Ask the user to restart Spark."}]}}),
            ));
        }
        Ok(Some(res.json().await?))
    }

    async fn start_headless(&mut self) -> bool {
        if self.started {
            return false;
        }
        self.started = true;
        let Ok(listener) = crate::bind(29890).await else { return false };
        let config = self.config.clone();
        let web = self.web_dir.clone();
        tokio::spawn(async move { crate::serve_on(listener, web, config).await });
        // serve_on writes the port file before accepting; give it a moment.
        for _ in 0..50 {
            tokio::time::sleep(Duration::from_millis(20)).await;
            if self.client.get(format!("{}/health", self.base())).send().await.is_ok() {
                return true;
            }
        }
        true
    }

    async fn handle(&mut self, mut msg: Value) -> Option<Value> {
        let method = msg["method"].as_str()?.to_string();
        let id = msg.get("id").cloned();
        match method.as_str() {
            "initialize" => {
                self.client_info = msg["params"]["clientInfo"].clone();
                let asked = msg["params"]["protocolVersion"].as_str().unwrap_or(mcp::PROTOCOL_VERSIONS[0]);
                let version = if mcp::PROTOCOL_VERSIONS.contains(&asked) { asked } else { mcp::PROTOCOL_VERSIONS[0] };
                return Some(json!({"jsonrpc": "2.0", "id": id?, "result": {
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": false}},
                    "serverInfo": {"name": "spark", "title": "Spark", "version": spark_core::VERSION},
                    "instructions": mcp::tools::INSTRUCTIONS,
                }}));
            }
            "tools/list" => {
                // Prefer the running server's list; fall back to the one built into this binary.
                let live = async {
                    let r = self
                        .client
                        .get(format!("{}/mcp/config.json", self.base()))
                        .bearer_auth(&self.config.token)
                        .send()
                        .await
                        .ok()?;
                    r.json::<Value>().await.ok()
                }
                .await;
                let tools = live
                    .map(|c| c["tools"].clone())
                    .filter(Value::is_array)
                    .unwrap_or_else(|| mcp::config_json()["tools"].clone());
                return Some(json!({"jsonrpc": "2.0", "id": id?, "result": {"tools": tools}}));
            }
            "ping" => return Some(json!({"jsonrpc": "2.0", "id": id?, "result": {}})),
            _ if id.is_none() => return None,
            _ => {}
        }
        if self.client_info.is_object() {
            msg["params"]["_meta"]["io.modelcontextprotocol/clientInfo"] = self.client_info.clone();
        }
        match self.post(&msg).await {
            Ok(reply) => reply,
            Err(e) if e.is_connect() && self.headless && self.start_headless().await => {
                self.post(&msg).await.ok().flatten()
            }
            Err(_) if method == "tools/call" => Some(
                json!({"jsonrpc": "2.0", "id": id, "result": {"isError": true, "content": [{"type": "text", "text": OFFLINE}]}}),
            ),
            Err(_) => Some(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32000, "message": OFFLINE}})),
        }
    }
}

/// Runs the relay on stdin/stdout until stdin closes.
pub async fn run(config: Config, headless: bool, web_dir: Option<PathBuf>) -> anyhow::Result<()> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(120)).build()?;
    let mut relay =
        Relay { config, client, session: None, client_info: Value::Null, headless, started: false, web_dir };
    if headless && relay.client.get(format!("{}/health", relay.base())).send().await.is_err() {
        relay.start_headless().await;
    }
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut out = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(batch)) => {
                let mut replies = Vec::new();
                for m in batch {
                    replies.extend(relay.handle(m).await);
                }
                (!replies.is_empty()).then(|| Value::Array(replies))
            }
            Ok(m) => relay.handle(m).await,
            Err(e) => Some(
                json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {e}")}}),
            ),
        };
        if let Some(r) = reply {
            out.write_all(format!("{r}\n").as_bytes()).await?;
            out.flush().await?;
        }
    }
    Ok(())
}
