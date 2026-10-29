//! The agent panel's runner: finds agent CLIs on PATH and runs one headless with Spark's MCP
//! attached, streaming its output to the UI. Spark never sees the agents' credentials; each CLI
//! uses its own login.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Mutex, OnceLock},
};

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::{
    connect::{Launch, status},
    hub::Tx,
};

/// (cli on PATH, display name, connect id it needs when it can't take MCP config by flag)
pub const CLIS: &[(&str, &str, Option<&str>)] = &[
    ("claude", "Claude Code", None),
    ("codex", "Codex", None),
    ("opencode", "OpenCode", Some("opencode")),
    ("gemini", "Gemini CLI", Some("gemini")),
    ("cursor-agent", "Cursor Agent", Some("cursor")),
];

fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    // Windows launchers carry an extension: codex.exe, or gemini.cmd from npm.
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT".into()).split(';').map(str::to_string).collect()
    } else {
        vec![String::new()]
    };
    std::env::split_paths(&path)
        .flat_map(|d| exts.iter().map(move |e| d.join(format!("{name}{e}"))))
        .find(|p| p.is_file() && is_executable(p))
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_: &Path) -> bool {
    true
}

/// Agent CLIs found on PATH, and whether Spark is in their config when they need it.
pub fn detect(home: &Path, config_dir: &Path) -> Vec<Value> {
    CLIS.iter()
        .filter_map(|(cli, name, needs)| {
            let path = which(cli)?;
            let ready = needs.is_none_or(|a| status(a, home, config_dir));
            Some(json!({"id": cli, "name": name, "path": path, "connected": ready, "needs": needs}))
        })
        .collect()
}

fn runs() -> &'static Mutex<HashMap<u64, tokio::process::Child>> {
    static R: OnceLock<Mutex<HashMap<u64, tokio::process::Child>>> = OnceLock::new();
    R.get_or_init(Mutex::default)
}

/// The command line for a headless run with Spark's MCP attached.
pub fn command(cli: &str, prompt: &str, launch: &Launch, temp: &Path) -> Result<Vec<String>, String> {
    let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    Ok(match cli {
        "claude" => {
            let cfg =
                json!({"mcpServers": {"spark": {"type": "stdio", "command": launch.command, "args": launch.args}}});
            let file = temp.join("spark-mcp.json");
            crate::config::write_private(&file, cfg.to_string().as_bytes()).map_err(|e| e.to_string())?;
            let mut v = args(&["-p", prompt, "--mcp-config"]);
            v.push(file.to_string_lossy().into());
            v.extend(args(&["--allowedTools", "mcp__spark", "--output-format", "stream-json", "--verbose"]));
            v
        }
        "codex" => {
            let mut v = args(&["exec", "--skip-git-repo-check", "-c"]);
            v.push(format!("mcp_servers.spark.command={}", serde_json::to_string(&launch.command).unwrap()));
            v.push("-c".into());
            v.push(format!("mcp_servers.spark.args={}", serde_json::to_string(&launch.args).unwrap()));
            v.push(prompt.into());
            v
        }
        "opencode" => args(&["run", prompt]),
        "gemini" => args(&["-p", prompt, "--yolo"]),
        "cursor-agent" => args(&["-p", prompt, "--output-format", "text", "--force"]),
        other => return Err(format!("unknown agent CLI `{other}`")),
    })
}

/// Starts a run and streams `{t:"run", runId, kind, line}` messages to `tx`.
pub fn start(cli: &str, prompt: &str, file_name: &str, file_id: &str, cwd: &Path, tx: Tx) -> Result<u64, String> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let exe = which(cli).ok_or_else(|| format!("`{cli}` is not on your PATH"))?;
    let full = format!(
        "You are working in the Spark design file \"{file_name}\" (fileId {file_id}). Use the spark MCP tools; read get_guide(\"spark-mcp-instructions\") first and pass this fileId.\n\n{prompt}"
    );
    let temp = std::env::temp_dir().join(format!("spark-run-{}", std::process::id()));
    std::fs::create_dir_all(&temp).map_err(|e| e.to_string())?;
    let argv = command(cli, &full, &Launch::current(), &temp)?;
    let mut cmd = tokio::process::Command::new(exe);
    // Its own process group, so Stop can end the shells and tools the agent starts as well.
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd
        .args(&argv)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("couldn't start {cli}: {e}"))?;
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    for (kind, pipe) in [
        ("out", child.stdout.take().map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Unpin + Send>)),
        ("err", child.stderr.take().map(|p| Box::new(p) as Box<dyn tokio::io::AsyncRead + Unpin + Send>)),
    ] {
        let Some(pipe) = pipe else { continue };
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(pipe).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = tx.send(json!({"t": "run", "runId": id, "kind": kind, "line": line}).to_string());
            }
        });
    }
    runs().lock().unwrap().insert(id, child);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let status = {
                let mut r = runs().lock().unwrap();
                match r.get_mut(&id).map(|c| c.try_wait()) {
                    Some(Ok(Some(s))) => {
                        r.remove(&id);
                        Some(s.code())
                    }
                    Some(Ok(None)) => continue,
                    _ => Some(None),
                }
            };
            // Let the output readers drain before announcing the end.
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            let _ = tx.send(json!({"t": "run", "runId": id, "kind": "done", "code": status.flatten()}).to_string());
            break;
        }
    });
    Ok(id)
}

pub fn stop(id: u64) -> bool {
    match runs().lock().unwrap().get_mut(&id) {
        Some(c) => {
            #[cfg(unix)]
            if let Some(pid) = c.id() {
                // SAFETY: plain syscall; the group id is the child's pid (see `process_group(0)`).
                unsafe { libc::killpg(pid as i32, libc::SIGKILL) };
            }
            #[cfg(windows)]
            if let Some(pid) = c.id() {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                let _ = std::process::Command::new("taskkill")
                    .args(["/T", "/F", "/PID", &pid.to_string()])
                    .creation_flags(CREATE_NO_WINDOW)
                    .status();
            }
            c.start_kill().is_ok()
        }
        None => false,
    }
}
