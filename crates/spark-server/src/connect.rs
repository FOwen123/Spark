//! `spark connect`: adds (or removes) a `spark` MCP entry in each agent's own config file, editing
//! in place so comments and formatting survive. Writes are idempotent and atomic, keep the file's
//! mode (0600 for new files), and refuse files that don't parse.

use std::path::{Path, PathBuf};

use jsonc_parser::cst::{CstInputValue, CstRootNode};
use serde::Serialize;
use serde_json::{Value, json};

use crate::config::write_private;

pub const AGENTS: &[(&str, &str)] = &[
    ("claude-code", "Claude Code"),
    ("codex", "Codex"),
    ("cursor", "Cursor"),
    ("vscode", "VS Code"),
    ("copilot", "GitHub Copilot CLI"),
    ("opencode", "OpenCode"),
    ("gemini", "Gemini CLI"),
    ("antigravity", "Antigravity"),
    ("claude-desktop", "Claude Desktop"),
];

pub fn agent_id(name: &str) -> Option<&'static str> {
    let n = name.trim().to_ascii_lowercase();
    let n = match n.as_str() {
        "claude" | "claudecode" | "claude_code" => "claude-code",
        "code" | "vs-code" | "visual-studio-code" => "vscode",
        "github-copilot" | "copilot-cli" => "copilot",
        "gemini-cli" => "gemini",
        "claudedesktop" => "claude-desktop",
        other => other,
    };
    AGENTS.iter().find(|(id, _)| *id == n).map(|(id, _)| *id)
}

/// The command agents run to reach Spark: this binary's `mcp` subcommand.
#[derive(Clone, Debug)]
pub struct Launch {
    pub command: String,
    pub args: Vec<String>,
}

impl Launch {
    pub fn current() -> Launch {
        let exe =
            std::env::current_exe().ok().and_then(|p| p.canonicalize().ok()).unwrap_or_else(|| PathBuf::from("spark"));
        Launch { command: exe.to_string_lossy().replace('\\', "/"), args: vec!["mcp".into()] }
    }
}

#[derive(Debug, Serialize, PartialEq)]
pub struct Change {
    pub agent: String,
    pub path: PathBuf,
    pub changed: bool,
    pub created: bool,
    /// Unified diff of the edit (empty when nothing changed).
    pub diff: String,
    pub note: Option<String>,
}

enum Format {
    /// JSON or JSONC; the entry lives at `keys` → "spark".
    Json(&'static [&'static str]),
    Toml,
}

fn vscode_user_dir(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/Code/User")
    } else if cfg!(windows) {
        // APPDATA belongs to the real home; an overridden home (--home, tests) keeps its own.
        let appdata = std::env::var_os("APPDATA").filter(|_| dirs::home_dir().as_deref() == Some(home));
        appdata.map(PathBuf::from).unwrap_or_else(|| home.join("AppData/Roaming")).join("Code/User")
    } else {
        home.join(".config/Code/User")
    }
}

/// The config file and format for an agent, relative to `home`.
fn target(agent: &str, home: &Path) -> Option<(PathBuf, Format)> {
    Some(match agent {
        "claude-code" => (home.join(".claude.json"), Format::Json(&["mcpServers"])),
        "codex" => (home.join(".codex/config.toml"), Format::Toml),
        "cursor" => (home.join(".cursor/mcp.json"), Format::Json(&["mcpServers"])),
        "vscode" => (vscode_user_dir(home).join("mcp.json"), Format::Json(&["servers"])),
        "copilot" => (home.join(".copilot/mcp-config.json"), Format::Json(&["mcpServers"])),
        "opencode" => {
            let dir = home.join(".config/opencode");
            let jsonc = dir.join("opencode.jsonc");
            (if jsonc.exists() { jsonc } else { dir.join("opencode.json") }, Format::Json(&["mcp"]))
        }
        "gemini" => (home.join(".gemini/settings.json"), Format::Json(&["mcpServers"])),
        "antigravity" => {
            // Current releases read ~/.gemini/config; earlier ones read ~/.gemini/antigravity.
            let old = home.join(".gemini/antigravity/mcp_config.json");
            let new = home.join(".gemini/config/mcp_config.json");
            (if old.exists() && !new.exists() { old } else { new }, Format::Json(&["mcpServers"]))
        }
        _ => return None,
    })
}

/// The `spark` entry each agent expects.
fn entry(agent: &str, l: &Launch) -> Value {
    match agent {
        "claude-code" | "vscode" => json!({"type": "stdio", "command": l.command, "args": l.args}),
        "copilot" => json!({"type": "local", "command": l.command, "args": l.args, "tools": ["*"]}),
        "opencode" => {
            let mut cmd = vec![l.command.clone()];
            cmd.extend(l.args.iter().cloned());
            json!({"type": "local", "command": cmd, "enabled": true})
        }
        _ => json!({"command": l.command, "args": l.args}),
    }
}

fn to_cst(v: &Value) -> CstInputValue {
    match v {
        Value::Null => CstInputValue::Null,
        Value::Bool(b) => CstInputValue::Bool(*b),
        Value::Number(n) => CstInputValue::Number(n.to_string()),
        Value::String(s) => CstInputValue::String(s.clone()),
        Value::Array(a) => CstInputValue::Array(a.iter().map(to_cst).collect()),
        Value::Object(o) => CstInputValue::Object(o.iter().map(|(k, v)| (k.clone(), to_cst(v))).collect()),
    }
}

/// Sets (or with `None`, removes) `keys…/spark` in a JSON/JSONC document, keeping everything else.
pub fn edit_json(text: &str, keys: &[&str], value: Option<&Value>) -> Result<String, String> {
    let source = if text.trim().is_empty() { "{}\n" } else { text };
    let root = CstRootNode::parse(source, &Default::default()).map_err(|e| format!("it doesn't parse as JSON: {e}"))?;
    let mut obj = match root.value() {
        None => root.object_value_or_set(),
        Some(_) => root.object_value().ok_or("its top level isn't a JSON object")?,
    };
    for key in keys {
        obj = match obj.get(key) {
            Some(prop) => prop.object_value().ok_or_else(|| format!("`{key}` isn't a JSON object"))?,
            None if value.is_none() => return Ok(text.to_string()),
            None => obj.object_value_or_set(key),
        };
    }
    match (obj.get("spark"), value) {
        (Some(prop), Some(v)) => {
            let current = prop
                .value()
                .and_then(|n| jsonc_parser::parse_to_serde_value(&n.to_string(), &Default::default()).ok().flatten());
            if current.as_ref() != Some(v) {
                prop.set_value(to_cst(v));
            }
        }
        (None, Some(v)) => {
            obj.append("spark", to_cst(v));
        }
        (Some(prop), None) => prop.remove(),
        (None, None) => {}
    }
    let mut out = root.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(if out.trim() == text.trim() { text.to_string() } else { out })
}

/// Sets (or removes) `[mcp_servers.spark]` in a TOML document, keeping formatting and comments.
pub fn edit_toml(text: &str, launch: Option<&Launch>) -> Result<String, String> {
    let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| format!("it doesn't parse as TOML: {e}"))?;
    match launch {
        Some(l) => {
            let servers = doc.entry("mcp_servers").or_insert_with(|| {
                let mut t = toml_edit::Table::new();
                t.set_implicit(true);
                toml_edit::Item::Table(t)
            });
            let servers = servers.as_table_like_mut().ok_or("`mcp_servers` isn't a table")?;
            let same = servers.get("spark").and_then(|s| s.as_table_like()).is_some_and(|t| {
                t.get("command").and_then(|c| c.as_str()) == Some(l.command.as_str())
                    && t.get("args")
                        .and_then(|a| a.as_array())
                        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect::<Vec<_>>())
                        == Some(l.args.clone())
            });
            if !same {
                let mut t = toml_edit::Table::new();
                t["command"] = toml_edit::value(l.command.clone());
                t["args"] = toml_edit::value(l.args.iter().collect::<toml_edit::Array>());
                servers.insert("spark", toml_edit::Item::Table(t));
            }
        }
        None => {
            if let Some(servers) = doc.get_mut("mcp_servers").and_then(|s| s.as_table_like_mut()) {
                servers.remove("spark");
            }
        }
    }
    let out = doc.to_string();
    Ok(if out == text { text.to_string() } else { out })
}

fn unified_diff(path: &Path, old: &str, new: &str) -> String {
    similar::TextDiff::from_lines(old, new)
        .unified_diff()
        .header(&path.display().to_string(), &path.display().to_string())
        .to_string()
}

/// Reads a config file as UTF-8 (dropping a BOM, decoding UTF-16LE as written by Windows tools).
fn read_text(path: &Path) -> Result<Option<String>, String> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("can't read {}: {e}", path.display())),
    };
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.chunks(2).map(|c| u16::from_le_bytes([c[0], *c.get(1).unwrap_or(&0)])).collect();
        return String::from_utf16(&units).map(Some).map_err(|_| format!("{} isn't valid UTF-16", path.display()));
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(&bytes);
    String::from_utf8(bytes.to_vec()).map(Some).map_err(|_| format!("{} isn't UTF-8 text", path.display()))
}

/// Connects (`remove = false`) or disconnects an agent. With `dry_run`, nothing is written.
pub fn apply(
    agent: &str,
    home: &Path,
    config_dir: &Path,
    launch: &Launch,
    remove: bool,
    dry_run: bool,
) -> Result<Change, String> {
    let id = agent_id(agent).ok_or_else(|| {
        format!(
            "unknown agent `{agent}`; choose one of: {}",
            AGENTS.iter().map(|(a, _)| *a).collect::<Vec<_>>().join(", ")
        )
    })?;
    if id == "claude-desktop" {
        return claude_desktop(config_dir, launch, remove, dry_run);
    }
    let (path, format) = target(id, home).unwrap();
    let _edit = crate::config::EDITS.lock().unwrap_or_else(|e| e.into_inner());
    let old = read_text(&path)?;
    let old_text = old.clone().unwrap_or_default();
    let refuse = |e: String| format!("refusing to edit {}: {e}", path.display());
    let new_text = match format {
        Format::Json(keys) => {
            edit_json(&old_text, keys, (!remove).then(|| entry(id, launch)).as_ref()).map_err(refuse)?
        }
        Format::Toml => edit_toml(&old_text, (!remove).then_some(launch)).map_err(refuse)?,
    };
    let changed = new_text != old_text && !(old.is_none() && remove);
    let diff = if changed { unified_diff(&path, &old_text, &new_text) } else { String::new() };
    if changed && !dry_run {
        write_private(&path, new_text.as_bytes()).map_err(|e| format!("can't write {}: {e}", path.display()))?;
    }
    Ok(Change { agent: id.into(), path, changed, created: old.is_none() && changed, diff, note: None })
}

/// The config file and the text to put in it for setting an agent up by hand: what `apply` writes
/// into a new file. `other` gives the common `mcpServers` shape with no file.
pub fn manual(agent: &str, home: &Path, launch: &Launch) -> Option<(Option<PathBuf>, String)> {
    let (path, format) = match agent {
        "other" => (None, Format::Json(&["mcpServers"])),
        _ => target(agent, home).map(|(p, f)| (Some(p), f))?,
    };
    let text = match format {
        Format::Json(keys) => edit_json("", keys, Some(&entry(agent, launch))),
        Format::Toml => edit_toml("", Some(launch)),
    };
    text.ok().map(|t| (path, t))
}

/// Whether the Claude Desktop app is installed, so connecting Claude can also offer its extension.
pub fn claude_desktop_installed(home: &Path) -> bool {
    if cfg!(target_os = "macos") {
        ["/Applications/Claude.app".into(), home.join("Applications/Claude.app")].iter().any(|p: &PathBuf| p.exists())
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").is_some_and(|d| Path::new(&d).join("AnthropicClaude").exists())
    } else {
        false
    }
}

/// Whether an agent's config already has a `spark` entry.
pub fn status(agent: &str, home: &Path, config_dir: &Path) -> bool {
    match (agent, target(agent, home)) {
        ("claude-desktop", _) => config_dir.join("spark.mcpb").exists(),
        (_, Some((path, Format::Json(keys)))) => read_text(&path).ok().flatten().is_some_and(|t| {
            let v: Option<Value> = jsonc_parser::parse_to_serde_value(&t, &Default::default()).ok().flatten();
            v.and_then(|mut v| {
                for k in keys {
                    v = v.get(*k)?.clone();
                }
                v.get("spark").cloned()
            })
            .is_some()
        }),
        (_, Some((path, Format::Toml))) => read_text(&path)
            .ok()
            .flatten()
            .and_then(|t| t.parse::<toml_edit::DocumentMut>().ok())
            .is_some_and(|d| d.get("mcp_servers").and_then(|s| s.get("spark")).is_some()),
        _ => false,
    }
}

/// Claude Desktop installs MCP servers from `.mcpb` bundles, opened with the system handler.
fn claude_desktop(config_dir: &Path, launch: &Launch, remove: bool, dry_run: bool) -> Result<Change, String> {
    let path = config_dir.join("spark.mcpb");
    if remove {
        let existed = path.exists();
        if existed && !dry_run {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        return Ok(Change {
            agent: "claude-desktop".into(),
            path,
            changed: existed,
            created: false,
            diff: String::new(),
            note: Some("Remove the Spark extension in Claude Desktop's settings, under Extensions.".into()),
        });
    }
    let manifest = json!({
        "manifest_version": "0.3",
        "name": "spark",
        "display_name": "Spark",
        "version": spark_core::VERSION,
        "description": "Read and edit Spark design files from Claude.",
        "author": {"name": "Spark contributors"},
        "server": {"type": "binary", "entry_point": launch.command, "mcp_config": {"command": launch.command, "args": launch.args}},
        "tools_generated": true,
    });
    let manifest = serde_json::to_string_pretty(&manifest).unwrap();
    if !dry_run {
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            zip.start_file("manifest.json", zip::write::SimpleFileOptions::default()).map_err(|e| e.to_string())?;
            std::io::Write::write_all(&mut zip, manifest.as_bytes()).map_err(|e| e.to_string())?;
            zip.finish().map_err(|e| e.to_string())?;
        }
        write_private(&path, buf.get_ref()).map_err(|e| e.to_string())?;
        if std::env::var_os("SPARK_NO_OPEN").is_none() {
            open::that_detached(&path).map_err(|e| format!("wrote {} but couldn't open it: {e}", path.display()))?;
        }
    }
    Ok(Change {
        agent: "claude-desktop".into(),
        path,
        changed: true,
        created: true,
        diff: format!("manifest.json\n{manifest}\n"),
        note: Some("Claude Desktop opens the bundle and asks to install the Spark extension.".into()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch() -> Launch {
        Launch { command: "/opt/spark/bin/spark".into(), args: vec!["mcp".into()] }
    }

    #[test]
    fn json_edits_keep_comments_and_are_idempotent() {
        let src = "{\n  // my settings\n  \"theme\": \"dark\", /* keep */\n  \"mcpServers\": {\n    \"other\": { \"command\": \"x\" },\n  },\n}\n";
        let v = entry("cursor", &launch());
        let out = edit_json(src, &["mcpServers"], Some(&v)).unwrap();
        assert!(out.contains("// my settings") && out.contains("/* keep */") && out.contains("\"other\""), "{out}");
        assert!(out.contains("\"spark\""));
        assert_eq!(edit_json(&out, &["mcpServers"], Some(&v)).unwrap(), out, "second run changes nothing");
        let removed = edit_json(&out, &["mcpServers"], None).unwrap();
        assert!(!removed.contains("spark") && removed.contains("// my settings"));
        assert!(edit_json("{ not json", &["mcpServers"], Some(&v)).is_err());
        assert!(edit_json("[1, 2]", &["mcpServers"], Some(&v)).is_err());
        assert!(edit_json("{\"mcpServers\": 3}", &["mcpServers"], Some(&v)).is_err());
        let fresh = edit_json("", &["mcpServers"], Some(&v)).unwrap();
        let parsed: Value = serde_json::from_str(&fresh).unwrap();
        assert_eq!(parsed["mcpServers"]["spark"]["args"][0], "mcp");
    }

    #[test]
    fn toml_edits_keep_formatting() {
        let src = "# Codex settings\nmodel = \"o4\"   # keep this comment\n\n[mcp_servers.other]\ncommand = \"x\"\n";
        let out = edit_toml(src, Some(&launch())).unwrap();
        assert!(out.starts_with("# Codex settings\nmodel = \"o4\"   # keep this comment\n"), "{out}");
        assert!(out.contains("[mcp_servers.spark]\ncommand = \"/opt/spark/bin/spark\"\nargs = [\"mcp\"]"), "{out}");
        assert_eq!(edit_toml(&out, Some(&launch())).unwrap(), out);
        let removed = edit_toml(&out, None).unwrap();
        assert!(!removed.contains("spark") && removed.contains("[mcp_servers.other]"));
        assert!(edit_toml("this is = = not toml", Some(&launch())).is_err());
    }

    #[test]
    fn apply_writes_atomically_with_private_mode() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let c = apply("claude", home, &home.join("cfg"), &launch(), false, true).unwrap();
        assert!(c.changed && c.created && c.diff.contains("+"));
        assert!(!home.join(".claude.json").exists(), "dry run writes nothing");
        let c = apply("claude-code", home, &home.join("cfg"), &launch(), false, false).unwrap();
        assert!(c.changed);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(home.join(".claude.json")).unwrap().permissions().mode() & 0o777, 0o600);
        }
        assert!(status("claude-code", home, &home.join("cfg")));
        let again = apply("claude-code", home, &home.join("cfg"), &launch(), false, false).unwrap();
        assert!(!again.changed);
        let gone = apply("claude-code", home, &home.join("cfg"), &launch(), true, false).unwrap();
        assert!(gone.changed && !status("claude-code", home, &home.join("cfg")));
        assert!(apply("nope", home, home, &launch(), false, true).is_err());
    }

    #[test]
    fn every_agent_has_a_target() {
        let dir = tempfile::tempdir().unwrap();
        for (id, _) in AGENTS {
            let c = apply(id, dir.path(), &dir.path().join("cfg"), &launch(), false, true).unwrap();
            assert!(c.changed, "{id}");
        }
    }
}
