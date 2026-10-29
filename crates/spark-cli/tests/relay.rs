use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
};

use serde_json::{Value, json};

fn relay(config_dir: &std::path::Path, lines: &[Value]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_spark"))
        .arg("mcp")
        .env("SPARK_CONFIG_DIR", config_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut out = BufReader::new(child.stdout.take().unwrap());
    let mut replies = Vec::new();
    for l in lines {
        writeln!(stdin, "{l}").unwrap();
        if l.get("id").is_some() {
            let mut buf = String::new();
            out.read_line(&mut buf).unwrap();
            replies.push(serde_json::from_str(&buf).unwrap());
        }
    }
    drop(stdin);
    child.wait().unwrap();
    replies
}

fn init() -> Value {
    json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-03-26", "capabilities": {}, "clientInfo": {"name": "codex", "version": "1"}}})
}

#[tokio::test(flavor = "multi_thread")]
async fn relays_to_the_running_server() {
    let dir = tempfile::tempdir().unwrap();
    let config = spark_server::Config::load(dir.path().join("cfg"), dir.path().join("files")).unwrap();
    let listener = spark_server::bind(0).await.unwrap();
    tokio::spawn(spark_server::serve_on(listener, None, config));
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let cfg = dir.path().join("cfg");
    let replies = tokio::task::spawn_blocking(move || {
        relay(
            &cfg,
            &[
                init(),
                json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
                json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
                json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "create_file", "arguments": {"name": "Relayed"}}}),
                json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "list_resources", "arguments": {}}}),
            ],
        )
    })
    .await
    .unwrap();
    assert_eq!(replies[0]["result"]["protocolVersion"], "2025-03-26");
    assert!(replies[1]["result"]["tools"].as_array().unwrap().len() > 20);
    let created: Value = serde_json::from_str(replies[2]["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(created["name"], "Relayed");
    assert!(replies[3]["result"]["content"][0]["text"].as_str().unwrap().contains("Relayed"));
}

#[test]
fn answers_locally_and_explains_when_spark_is_closed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path()).unwrap();
    // A port nothing listens on.
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    std::fs::write(dir.path().join("port"), port.to_string()).unwrap();
    let replies = relay(
        dir.path(),
        &[
            init(),
            json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "get_basic_info", "arguments": {}}}),
        ],
    );
    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "spark");
    assert!(
        replies[1]["result"]["tools"].as_array().unwrap().iter().any(|t| t["name"] == "write_html"),
        "bundled tool list"
    );
    assert_eq!(replies[2]["result"]["isError"], true);
    assert!(replies[2]["result"]["content"][0]["text"].as_str().unwrap().contains("Spark isn't running"));
}

#[test]
fn connect_dry_run_prints_a_diff_and_writes_nothing() {
    let home = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_spark"))
        .args(["connect", "codex", "--dry-run", "--home"])
        .arg(home.path())
        .env("SPARK_CONFIG_DIR", home.path().join("cfg"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("Would edit") && text.contains("+[mcp_servers.spark]"), "{text}");
    assert!(!home.path().join(".codex/config.toml").exists());
}
