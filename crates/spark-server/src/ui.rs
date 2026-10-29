//! The `/ui` WebSocket: one connection per UI window, carrying snapshots and patches down and
//! intents, selection and measured rects up.

use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use spark_core::{NodeId, Rect};

use crate::{
    commands,
    hub::{ClientId, Hub, Tx},
    views,
};

pub async fn client(socket: WebSocket, hub: Arc<Hub>) {
    let (mut sink, mut stream) = socket.split();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let id = hub.add_client(tx.clone());
    let writer = tokio::spawn(async move {
        while let Some(m) = rx.recv().await {
            if sink.send(Message::Text(m.into())).await.is_err() {
                break;
            }
        }
    });
    while let Some(Ok(msg)) = stream.next().await {
        if let Message::Text(text) = msg {
            match serde_json::from_str::<Value>(&text) {
                Ok(v) => handle(&hub, id, &tx, v),
                Err(e) => drop(tx.send(json!({"t": "error", "message": format!("bad message: {e}")}).to_string())),
            }
        }
    }
    hub.remove_client(id);
    writer.abort();
}

fn reply(tx: &Tx, rid: &Value, result: Result<Value, String>) {
    let msg = match result {
        Ok(v) => json!({"t": "reply", "rid": rid, "ok": true, "result": v}),
        Err(e) => json!({"t": "reply", "rid": rid, "ok": false, "error": e}),
    };
    let _ = tx.send(msg.to_string());
}

fn handle(hub: &Arc<Hub>, client: ClientId, tx: &Tx, msg: Value) {
    let rid = msg["rid"].clone();
    let file = msg["file"].as_str().unwrap_or_default();
    let with_doc = |f: &mut dyn FnMut(&mut crate::hub::OpenDoc) -> Result<Value, String>| -> Result<Value, String> {
        let doc = hub.get(file).ok_or_else(|| format!("file {file} is not open"))?;
        let mut d = doc.lock().unwrap();
        f(&mut d)
    };
    match msg["t"].as_str().unwrap_or_default() {
        "hello" => reply(
            tx,
            &rid,
            Ok(
                json!({"recent": hub.config.recent(), "filesDir": hub.config.files_dir, "agents": hub.agents_json()["agents"], "native": hub.native.get().is_some()}),
            ),
        ),
        "reveal" => reply(tx, &rid, hub.reveal(msg["ref"].as_str().unwrap_or_default()).map(|id| json!({"file": id}))),
        "native" => {
            if let Some(f) = hub.native.get() {
                f(msg["cmd"].as_str().unwrap_or_default());
            }
        }
        "view" => hub.set_viewing(client, msg["file"].as_str()),
        "connectStatus" => {
            let home = crate::home_dir();
            let launch = crate::connect::Launch::current();
            let list: Vec<Value> = crate::connect::AGENTS
                .iter()
                .chain([&("other", "Other agents")])
                .map(|(id, name)| {
                    let manual = crate::connect::manual(id, &home, &launch);
                    json!({
                        "id": id, "name": name, "connected": crate::connect::status(id, &home, &hub.config.dir),
                        "installed": *id != "claude-desktop" || crate::connect::claude_desktop_installed(&home),
                        // Shown, not used: `~` keeps the path short. The snippet keeps the real command.
                        "path": manual.as_ref().and_then(|m| m.0.as_ref()).map(|p| match p.strip_prefix(&home) {
                            Ok(rel) => format!("~/{}", rel.display()),
                            Err(_) => p.display().to_string(),
                        }),
                        "snippet": manual.map(|m| m.1),
                    })
                })
                .collect();
            reply(tx, &rid, Ok(json!(list)));
        }
        "connect" => {
            let r = crate::connect::apply(
                msg["agent"].as_str().unwrap_or_default(),
                &crate::home_dir(),
                &hub.config.dir,
                &crate::connect::Launch::current(),
                msg["remove"].as_bool().unwrap_or(false),
                msg["dryRun"].as_bool().unwrap_or(false),
            );
            reply(tx, &rid, r.map(|c| json!(c)));
        }
        "settingsGet" => {
            let s = crate::settings::Settings::load(&hub.config);
            let providers: Vec<Value> = crate::generate::PROVIDERS
                .iter()
                .map(|p| {
                    let c = s.images.providers.get(p.id()).cloned().unwrap_or_default();
                    json!({
                        "id": p.id(), "name": p.name(), "needsKey": p.needs_key(), "hasKey": crate::secrets::get(p.id()).is_some(),
                        "enabled": c.enabled, "model": c.model, "baseUrl": c.base_url, "costPerImage": c.cost_per_image,
                        "defaultModel": p.default_model(), "defaultBase": p.default_base(), "defaultCost": p.default_cost(),
                    })
                })
                .collect();
            reply(
                tx,
                &rid,
                Ok(json!({
                    "grantedDirs": s.granted_dirs, "perCallCap": s.images.per_call_cap, "monthlyBudget": s.images.monthly_budget,
                    "spent": crate::generate::spent(&s), "providers": providers,
                })),
            );
        }
        "settingsSet" => {
            let r = (|| {
                let _edit = crate::config::EDITS.lock().unwrap_or_else(|e| e.into_inner());
                let mut s = crate::settings::Settings::load(&hub.config);
                if let Some(dirs) = msg["grantedDirs"].as_array() {
                    s.granted_dirs = dirs
                        .iter()
                        .filter_map(|d| d.as_str())
                        .filter(|d| !d.trim().is_empty())
                        .map(std::path::PathBuf::from)
                        .collect();
                    if let Some(bad) = s.granted_dirs.iter().find(|d| !d.is_absolute() || !d.is_dir()) {
                        return Err(format!("{} is not a folder", bad.display()));
                    }
                }
                if let Some(cap) = msg["perCallCap"].as_u64() {
                    s.images.per_call_cap = cap.clamp(1, 100) as u32;
                }
                if msg.get("monthlyBudget").is_some() {
                    s.images.monthly_budget = msg["monthlyBudget"].as_f64().filter(|b| *b >= 0.0);
                }
                if let (Some(id), Some(p)) = (msg["provider"].as_str(), msg["providerSettings"].as_object()) {
                    crate::generate::provider(id).ok_or("unknown provider")?;
                    let entry = s.images.providers.entry(id.to_string()).or_default();
                    let text = |k: &str| {
                        p.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
                    };
                    if p.contains_key("enabled") {
                        entry.enabled = p["enabled"].as_bool().unwrap_or(false);
                    }
                    if p.contains_key("model") {
                        entry.model = text("model");
                    }
                    if p.contains_key("baseUrl") {
                        entry.base_url = text("baseUrl");
                    }
                    if p.contains_key("costPerImage") {
                        entry.cost_per_image = p["costPerImage"].as_f64();
                    }
                    if let Some(key) = p.get("key").and_then(Value::as_str) {
                        crate::secrets::set(id, key.trim())?;
                    }
                }
                s.save(&hub.config).map_err(|e| e.to_string())
            })();
            reply(tx, &rid, r.map(|_| Value::Null));
        }
        "export" => {
            let hub = hub.clone();
            let tx = tx.clone();
            let file = file.to_string();
            tokio::spawn(async move {
                let r = async {
                    let doc = hub.get(&file).ok_or("file not open")?;
                    let ids: Vec<NodeId> =
                        msg["ids"].as_array().into_iter().flatten().filter_map(|v| v.as_str()?.parse().ok()).collect();
                    let presets = crate::export::presets(&msg["presets"])?;
                    let dir = crate::export::downloads_dir();
                    let mut paths = Vec::new();
                    if msg["combined"].as_bool().unwrap_or(false) {
                        let (bytes, name) = {
                            let mut d = doc.lock().unwrap();
                            (crate::export::combined_pdf(&mut d.editor.doc, &ids)?, d.editor.doc.name.clone())
                        };
                        paths.push(crate::export::save(&dir, &name, "pdf", &bytes)?);
                    } else {
                        for id in ids {
                            paths.extend(crate::export::export_presets(Some(&hub), &doc, id, &presets, &dir).await?);
                        }
                    }
                    Ok::<_, String>(json!({"paths": paths}))
                }
                .await;
                reply(&tx, &msg["rid"], r);
            });
        }
        "agentDetect" => reply(tx, &rid, Ok(json!(crate::runner::detect(&crate::home_dir(), &hub.config.dir)))),
        "agentRun" => {
            let r = (|| {
                let doc = hub.get(file).ok_or("open a file first")?;
                let (name, path) = {
                    let d = doc.lock().unwrap();
                    (d.editor.doc.name.clone(), d.path.clone())
                };
                let cwd = path.parent().map(std::path::Path::to_path_buf).unwrap_or_else(std::env::temp_dir);
                crate::runner::start(
                    msg["agent"].as_str().unwrap_or_default(),
                    msg["prompt"].as_str().unwrap_or_default(),
                    &name,
                    file,
                    &cwd,
                    tx.clone(),
                )
            })();
            reply(tx, &rid, r.map(|id| json!({"runId": id})));
        }
        "agentStop" => reply(tx, &rid, Ok(json!(crate::runner::stop(msg["runId"].as_u64().unwrap_or(0))))),
        "rendered" => {
            use base64::Engine;
            let call = msg["callId"].as_u64().unwrap_or(0);
            let result = if msg["ok"].as_bool().unwrap_or(false) {
                let data = msg["data"].as_str().unwrap_or_default();
                base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| e.to_string())
            } else {
                Err(msg["error"].as_str().unwrap_or("render failed").to_string())
            };
            hub.render_done(call, result);
        }
        "create" => {
            let r = hub.create(msg["name"].as_str().unwrap_or("Untitled")).map(|d| d.lock().unwrap().file_info());
            reply(tx, &rid, r);
        }
        "open" => {
            let r = hub.open_ref(msg["ref"].as_str().unwrap_or(file)).map(|d| {
                let mut d = d.lock().unwrap();
                d.subscribers.insert(client, tx.clone());
                views::snapshot(&mut d)
            });
            reply(tx, &rid, r);
        }
        "close" => {
            if let Some(d) = hub.get(file) {
                d.lock().unwrap().subscribers.remove(&client);
            }
            reply(tx, &rid, Ok(Value::Null));
        }
        "select" => {
            let r = with_doc(&mut |d| {
                let ids: Vec<NodeId> =
                    msg["ids"].as_array().into_iter().flatten().filter_map(|v| v.as_str()?.parse().ok()).collect();
                d.editor.selection = ids.into_iter().filter(|i| d.editor.doc.node(*i).is_some()).collect();
                let others = json!({"t": "selection", "file": file, "selection": d.editor.selection});
                for (c, ctx) in &d.subscribers {
                    if *c != client {
                        let _ = ctx.send(others.to_string());
                    }
                }
                Ok(Value::Null)
            });
            reply(tx, &rid, r);
        }
        "page" => {
            let r = with_doc(&mut |d| {
                let page = msg["page"]
                    .as_str()
                    .and_then(|p| p.parse().ok())
                    .filter(|p| d.editor.doc.page(*p).is_some_and(|pg| !pg.divider));
                d.editor.page = Some(page.ok_or("unknown page")?);
                d.editor.selection.clear();
                Ok(Value::Null)
            });
            reply(tx, &rid, r);
        }
        "rects" => {
            let _ = with_doc(&mut |d| {
                if let Some(map) = msg["rects"].as_object() {
                    for (id, r) in map {
                        let (Ok(id), Some(a)) = (id.parse::<NodeId>(), r.as_array()) else { continue };
                        let n: Vec<f64> = a.iter().filter_map(Value::as_f64).collect();
                        if let [x, y, w, h] = n[..] {
                            d.editor.doc.set_measured(id, Rect { x, y, w, h });
                        }
                    }
                }
                Ok(Value::Null)
            });
        }
        "do" if msg["cmd"]["type"] == "pasteHtml" => {
            // Images load before the document is locked, so this runs as its own task.
            let hub = hub.clone();
            let tx = tx.clone();
            let file = file.to_string();
            tokio::spawn(async move {
                let cmd = &msg["cmd"];
                let html = cmd["html"].as_str().unwrap_or_default().to_string();
                let settings = crate::settings::Settings::load(&hub.config);
                let loaded = crate::images::prefetch(&crate::images::image_urls(&html), &settings).await;
                let r = (|| {
                    let doc = hub.get(&file).ok_or_else(|| format!("file {file} is not open"))?;
                    let mut d = doc.lock().unwrap();
                    let parent: spark_core::Parent = cmd["parent"]
                        .as_str()
                        .unwrap_or_default()
                        .parse()
                        .map_err(|e: spark_core::id::IdError| e.to_string())?;
                    let at = cmd["x"].as_f64().zip(cmd["y"].as_f64());
                    let (result, ops) =
                        commands::paste_html(&mut d, &html, crate::edit::Placement { parent, at }, &loaded)?;
                    d.commit_ops(&ops);
                    Ok(result)
                })();
                reply(&tx, &msg["rid"], r);
            });
        }
        "do" => {
            let r = with_doc(&mut |d| {
                let (result, ops) = commands::run(d, &msg["cmd"])?;
                d.commit_ops(&ops);
                Ok(result)
            });
            reply(tx, &rid, r);
        }
        other => reply(tx, &rid, Err(format!("unknown message `{other}`"))),
    }
}
