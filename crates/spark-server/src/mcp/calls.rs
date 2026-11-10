//! Tool implementations. Every file-scoped result starts with a header naming the file and the
//! token hash; every write runs as one undo transaction.

use std::collections::BTreeSet;

use base64::Engine;
use serde_json::{Map, Value, json};
use spark_core::{
    Document, ImageStatus, Node, NodeId, NodeKind, Op, PageId, Parent, Rect,
    codegen::{self, CodeOpts, Format, node_css},
    color::{Color, var_name},
    css::{apply_decl, camel},
    node::Page,
    style::{Fill, Fit, Len, Size},
    tokens::{Token, resolve, to_css, token_type, tokens_hash, validate_name},
};

use super::tools;
use crate::{
    AppState, commands, edit,
    hub::{DocRef, OpenDoc},
    images,
    settings::Settings,
};

type Content = Vec<Value>;
type R<T> = Result<T, String>;

// ---- helpers -----------------------------------------------------------------------------------

fn s<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str).filter(|v| !v.is_empty())
}

fn req<'a>(args: &'a Value, key: &str) -> R<&'a str> {
    s(args, key).ok_or_else(|| format!("`{key}` is required"))
}

fn node_id(v: &str) -> R<NodeId> {
    v.parse().map_err(|_| format!("`{v}` is not a node id"))
}

fn ids_arg(args: &Value, key: &str) -> R<Vec<NodeId>> {
    args.get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| format!("`{key}` must be a list of node ids"))?
        .iter()
        .map(|v| node_id(v.as_str().unwrap_or_default()))
        .collect()
}

fn text(v: impl Into<String>) -> Value {
    json!({"type": "text", "text": v.into()})
}

fn header(open: &OpenDoc) -> Value {
    text(json!({"file": open.file_info(), "contentHash": {"tokens": tokens_hash(&open.editor.doc.tokens)}}).to_string())
}

fn ok_json(open: &OpenDoc, mut payload: Value) -> R<Content> {
    if let (Some(e), Some(p)) = (&open.save_error, payload.as_object_mut()) {
        p.insert(
            "warning".into(),
            json!(format!("Spark has this change but couldn't save the file to disk ({e}). Tell the user.")),
        );
    }
    Ok(vec![header(open), text(serde_json::to_string_pretty(&payload).unwrap())])
}

fn doc_for(app: &AppState, args: &Value) -> R<DocRef> {
    match s(args, "fileId") {
        Some(r) => app.hub.open_ref(r),
        None => {
            app.hub.viewed().ok_or_else(|| "No file is open in Spark. Ask the user to open one, or pass fileId.".into())
        }
    }
}

fn live(doc: &Document, id: NodeId) -> R<&Node> {
    doc.node(id).filter(|_| doc.is_live(id)).ok_or_else(|| format!("node {id} not found"))
}

fn page_for(open: &OpenDoc, args: &Value) -> R<PageId> {
    let doc = &open.editor.doc;
    match s(args, "pageId") {
        Some(p) => {
            let id: PageId = p.parse().map_err(|_| format!("`{p}` is not a page id"))?;
            match doc.page(id) {
                Some(pg) if !pg.divider => Ok(id),
                Some(_) => Err(format!("{p} is a divider, not a page")),
                None => Err(format!("page {p} not found")),
            }
        }
        None => open
            .editor
            .page
            .filter(|p| doc.page(*p).is_some())
            .or(doc.first_page())
            .ok_or_else(|| "the file has no pages".into()),
    }
}

/// A node's world rect: measured, or derived from fixed px styles for unmeasured artboards.
pub fn rect_of(doc: &Document, id: NodeId) -> Option<Rect> {
    if let Some(r) = doc.measured.get(&id) {
        return Some(*r);
    }
    let n = doc.node(id)?;
    if !matches!(doc.parent(id), Some(Parent::Page(_))) {
        return None;
    }
    let px = |s: &Option<Size>| match s {
        Some(Size::Fixed(Len::Px(v))) => Some(*v),
        _ => None,
    };
    let x = n.style.left.as_ref().and_then(Len::px).unwrap_or(0.0);
    let y = n.style.top.as_ref().and_then(Len::px).unwrap_or(0.0);
    Some(Rect { x, y, w: px(&n.style.width)?, h: px(&n.style.height).unwrap_or(0.0) })
}

fn round(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn kind_name(k: NodeKind) -> &'static str {
    match k {
        NodeKind::Frame => "Frame",
        NodeKind::Rect => "Rectangle",
        NodeKind::Text => "Text",
        NodeKind::Svg => "SVG",
        NodeKind::Vector => "Vector",
    }
}

fn summary(doc: &Document, id: NodeId) -> Value {
    let n = doc.node(id).unwrap();
    let r = doc.measured.get(&id);
    json!({
        "id": id, "name": n.name, "component": kind_name(n.kind),
        "width": r.map(|r| round(r.w)), "height": r.map(|r| round(r.h)),
        "worldX": r.map(|r| round(r.x)), "worldY": r.map(|r| round(r.y)),
        "childCount": doc.children(Parent::Node(id)).count(),
    })
}

/// Runs a write as one undo step. On error everything is rolled back.
fn write<T>(open: &mut OpenDoc, f: impl FnOnce(&mut OpenDoc) -> R<(T, Vec<Op>)>) -> R<(T, Vec<Op>)> {
    // A gesture the user is in the middle of stays out of the agent's undo step.
    let gesture = open.editor.suspend_group();
    open.editor.begin();
    let result = match f(open) {
        Ok((v, ops)) => {
            open.editor.commit();
            open.commit_ops(&ops);
            Ok((v, ops))
        }
        Err(e) => {
            let undo = open.editor.cancel();
            open.commit_ops(&undo);
            Err(e)
        }
    };
    open.editor.resume_group(gesture);
    result
}

fn tx(open: &mut OpenDoc, ops: Vec<Op>) -> R<Vec<Op>> {
    open.editor.transact(ops, None).map_err(|e| e.to_string())
}

/// Artboards containing the given nodes (for presence indicators).
fn boards(doc: &Document, ids: &[NodeId]) -> Vec<NodeId> {
    let set: BTreeSet<NodeId> = ids.iter().filter_map(|id| doc.artboard_of(*id)).collect();
    set.into_iter().collect()
}

fn glob(pattern: &str, text: &str) -> bool {
    let p = pattern.to_lowercase();
    let t = text.to_lowercase();
    if !p.contains('*') {
        return t.contains(&p);
    }
    let parts: Vec<&str> = p.split('*').collect();
    let mut pos = 0;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match t[pos..].find(part) {
            Some(at) if i == 0 && at != 0 => return false,
            Some(at) => pos += at + part.len(),
            None => return false,
        }
    }
    parts.last().is_none_or(|l| l.is_empty() || t.ends_with(l))
}

// ---- dispatch ----------------------------------------------------------------------------------

pub async fn call(app: &AppState, session: &str, name: &str, args: &Value) -> R<Content> {
    match name {
        "get_guide" => {
            let topic = req(args, "topic")?;
            tools::guide(topic).map(|g| vec![text(g)]).ok_or_else(|| {
                format!("no guide `{topic}`; topics: spark-mcp-instructions, image-generation, mobile-status-bar")
            })
        }
        "list_resources" => list_resources(app, args),
        "create_file" => create_file(app, args),
        "open_file" => {
            let doc = app.hub.open_ref(req(args, "fileId")?)?;
            let id = doc.lock().unwrap().id().to_string();
            app.hub.broadcast_all(&json!({"t": "openTab", "file": id, "page": s(args, "pageId")}));
            let mut d = doc.lock().unwrap();
            if let Some(p) = s(args, "pageId").and_then(|p| p.parse().ok()).filter(|p| d.editor.doc.page(*p).is_some())
                && !app.hub.is_viewed(&id)
            {
                d.editor.page = Some(p);
            }
            basic_info(&d, args)
        }
        "rename_resource" => {
            let doc = app.hub.open_ref(req(args, "resourceId")?)?;
            let mut d = doc.lock().unwrap();
            let name = req(args, "name")?.to_string();
            write(&mut d, |o| Ok(((), tx(o, vec![Op::SetFileName { name }])?)))?;
            app.hub.config.remember(&d.editor.doc.id.clone(), &d.editor.doc.name.clone(), &d.path.clone());
            ok_json(&d, json!({"id": d.id(), "name": d.editor.doc.name, "type": "file"}))
        }
        "get_screenshot" => screenshot(app, session, args).await,
        "write_html" => write_html(app, session, args).await,
        "insert_image" => insert_image(app, session, args).await,
        "get_font_family_info" => font_info(app, args).await,
        "fulfill_image_request" => fulfill(app, session, args).await,
        "export" => export(app, args).await,
        "export_combined_pdf" => {
            let doc = doc_for(app, args)?;
            let ids = ids_arg(args, "nodeIds")?;
            let (bytes, name, head) = {
                let mut d = doc.lock().unwrap();
                for id in &ids {
                    live(&d.editor.doc, *id)?;
                }
                let name = s(args, "name").map(str::to_string).unwrap_or_else(|| d.editor.doc.name.clone());
                (crate::export::combined_pdf(&mut d.editor.doc, &ids)?, name, header(&d))
            };
            let dir = s(args, "dir").map(std::path::PathBuf::from).unwrap_or_else(crate::export::downloads_dir);
            let path = crate::export::save(&dir, &name, "pdf", &bytes)?;
            Ok(vec![head, text(serde_json::to_string_pretty(&json!({"path": path, "pages": ids.len()})).unwrap())])
        }
        _ => {
            let doc = doc_for(app, args)?;
            let mut d = doc.lock().unwrap();
            file_tool(app, session, &mut d, name, args)
        }
    }
}

fn file_tool(app: &AppState, session: &str, d: &mut OpenDoc, name: &str, args: &Value) -> R<Content> {
    let file = d.id().to_string();
    match name {
        "get_basic_info" => basic_info(d, args),
        "get_selection" => {
            let doc = &d.editor.doc;
            let nodes: Vec<Value> = d
                .editor
                .selection
                .iter()
                .filter(|id| doc.node(**id).is_some())
                .map(|id| {
                    let mut v = summary(doc, *id);
                    v["artboardId"] = json!(doc.artboard_of(*id));
                    v
                })
                .collect();
            ok_json(d, json!({"selectedNodes": nodes, "count": nodes.len()}))
        }
        "get_node_info" => {
            let id = node_id(req(args, "nodeId")?)?;
            let doc = &d.editor.doc;
            let n = live(doc, id)?;
            let r = doc.measured.get(&id);
            let parent = doc.parent(id).unwrap();
            let pr = match parent {
                Parent::Node(p) => doc.measured.get(&p).copied(),
                Parent::Page(_) => Some(Rect::default()),
            };
            let kids: Vec<NodeId> = doc.children(Parent::Node(id)).collect();
            let mut info = json!({
                "id": id, "name": n.name, "component": kind_name(n.kind),
                "width": r.map(|r| round(r.w)), "height": r.map(|r| round(r.h)),
                "worldX": r.map(|r| round(r.x)), "worldY": r.map(|r| round(r.y)),
                "x": r.zip(pr).map(|(r, p)| round(r.x - p.x)), "y": r.zip(pr).map(|(r, p)| round(r.y - p.y)),
                "isVisible": !n.hidden, "isLocked": n.locked,
                "parentId": parent.to_string(), "childIds": kids, "childCount": kids.len(),
                "artboardId": doc.artboard_of(id),
            });
            if let Some(t) = &n.text {
                info["textContent"] = json!(t);
            }
            if n.kind == NodeKind::Vector {
                info["tag"] = json!(n.tag);
            }
            if let Some(img) = &n.image {
                info["imageGeneration"] = json!({"status": ImageStatus::of(img).as_str(), "output": "raster", "requestId": img["id"], "error": img["error"]});
            }
            app.hub.agent_activity(session, &file, &[], &[id], false);
            ok_json(d, info)
        }
        "get_children" => {
            let raw = req(args, "nodeId")?;
            let doc = &d.editor.doc;
            let parent: Parent = if raw.starts_with("p-") {
                Parent::Page(raw.parse().map_err(|_| "bad page id")?)
            } else {
                Parent::Node(node_id(raw)?)
            };
            if let Parent::Node(n) = parent {
                live(doc, n)?;
            }
            let pr = match parent {
                Parent::Node(p) => doc.measured.get(&p).copied(),
                Parent::Page(_) => Some(Rect::default()),
            };
            let list: Vec<Value> = doc
                .children(parent)
                .filter(|c| doc.node(*c).is_some())
                .map(|c| {
                    let mut v = summary(doc, c);
                    let r = doc.measured.get(&c);
                    v["x"] = json!(r.zip(pr).map(|(r, p)| round(r.x - p.x)));
                    v["y"] = json!(r.zip(pr).map(|(r, p)| round(r.y - p.y)));
                    v
                })
                .collect();
            ok_json(d, json!(list))
        }
        "get_tree_summary" => {
            let id = node_id(req(args, "nodeId")?)?;
            let depth = args["depth"].as_u64().unwrap_or(3).clamp(1, 10) as usize;
            let doc = &d.editor.doc;
            live(doc, id)?;
            let mut out = String::new();
            tree_lines(doc, id, 0, depth, &mut out);
            app.hub.agent_activity(session, &file, &[], &[id], false);
            ok_json(d, json!({"summary": out, "nodeId": id, "depth": depth}))
        }
        "get_jsx" => {
            let doc = &d.editor.doc;
            let ids: Vec<NodeId> = match (s(args, "nodeId"), args.get("nodeIds")) {
                (Some(n), _) => vec![node_id(n)?],
                (None, Some(_)) => ids_arg(args, "nodeIds")?,
                _ => return Err("`nodeId` is required".into()),
            };
            for id in &ids {
                live(doc, *id)?;
            }
            let port = app.port;
            let url = move |h: &str| format!("http://127.0.0.1:{port}/assets/{h}");
            let opts = CodeOpts {
                format: if args["format"] == "tailwind" { Format::Tailwind } else { Format::Jsx },
                include_ids: args["includeNodeIds"].as_bool().unwrap_or(true),
                root_positioning: args["includeRootPositioning"].as_bool().unwrap_or(false),
                asset_url: &url,
            };
            let jsx = codegen::code(doc, &ids, &opts);
            app.hub.agent_activity(session, &file, &[], &ids, false);
            Ok(vec![header(d), text(jsx)])
        }
        "get_computed_styles" => {
            let ids = ids_arg(args, "nodeIds")?;
            let doc = &d.editor.doc;
            let mut styles = Map::new();
            for id in &ids {
                live(doc, *id)?;
                let mut m: Map<String, Value> =
                    node_css(doc, *id).into_iter().map(|(k, v)| (camel(&k), json!(v))).collect();
                if let Some(r) = doc.measured.get(id) {
                    m.insert("measuredWidth".into(), json!(round(r.w)));
                    m.insert("measuredHeight".into(), json!(round(r.h)));
                }
                styles.insert(id.to_string(), Value::Object(m));
            }
            app.hub.agent_activity(session, &file, &[], &ids, false);
            ok_json(d, json!({"styles": styles}))
        }
        "find_nodes" => find_nodes(d, args),
        "list_image_requests" => {
            let doc = &d.editor.doc;
            let mut list = Vec::new();
            for p in &doc.pages {
                for id in doc.descendants(Parent::Page(p.id)) {
                    let n = doc.node(id).unwrap();
                    let Some(img) = &n.image else { continue };
                    if ImageStatus::of(img) == ImageStatus::Pending {
                        let r = doc.measured.get(&id);
                        list.push(json!({
                            "requestId": img["id"], "nodeId": id, "name": n.name, "prompt": img["prompt"],
                            "aspectRatio": img["aspectRatio"], "referenceNodes": img["referenceNodes"],
                            "width": r.map(|r| round(r.w)), "height": r.map(|r| round(r.h)), "pageId": p.id,
                        }));
                    }
                }
            }
            ok_json(d, json!({"requests": list, "count": list.len()}))
        }
        "get_fill_image" => fill_image(d, args),
        "get_tokens" => get_tokens(d, args),
        "create_tokens" => create_tokens(d, args),
        "set_tokens" => set_tokens(d, args),
        "create_page" => {
            let name = s(args, "name").map(str::to_string);
            let icon = s(args, "icon").map(str::to_string);
            let after = d.editor.page.and_then(|p| d.editor.doc.pages.iter().position(|x| x.id == p)).map(|i| i + 1);
            let (id, _) = write(d, |o| {
                let id = o.editor.doc.alloc_page();
                let n = o.editor.doc.pages.iter().filter(|p| !p.divider).count() + 1;
                let page = Page { id, name: name.unwrap_or(format!("Page {n}")), icon, canvas: None, divider: false };
                Ok((id, tx(o, vec![Op::InsertPage { page, index: after.unwrap_or(usize::MAX) }])?))
            })?;
            ok_json(d, json!({"pageId": id}))
        }
        "update_pages" => {
            let updates = args["updates"].as_array().cloned().ok_or("`updates` is required")?;
            write(d, |o| {
                let mut ops = Vec::new();
                for u in &updates {
                    let id: PageId = u["pageId"].as_str().unwrap_or_default().parse().map_err(|_| "bad pageId")?;
                    if let Some(n) = u.get("name") {
                        ops.push(Op::SetPage { id, prop: "name".into(), value: n.clone() });
                    }
                    if let Some(i) = u.get("icon") {
                        ops.push(Op::SetPage { id, prop: "icon".into(), value: i.clone() });
                    }
                }
                Ok(((), tx(o, ops)?))
            })?;
            ok_json(d, json!({"pages": d.editor.doc.pages}))
        }
        "arrange_pages" => arrange_pages(d, args),
        "create_artboard" => {
            let (id, _) = write(d, |o| create_artboard(o, args))?;
            app.hub.agent_activity(session, &file, &[id], &[id], true);
            ok_json(d, json!({"id": id, "name": d.editor.doc.node(id).map(|n| n.name.clone())}))
        }
        "update_styles" => {
            let updates = args["updates"].as_array().cloned().ok_or("`updates` is required")?;
            let mut touched = Vec::new();
            let ((ignored, count), _) = write(d, |o| {
                let mut ignored = Vec::new();
                let mut ops = Vec::new();
                let mut count = 0;
                for u in &updates {
                    let styles = u["styles"].as_object().cloned().ok_or("each update needs `styles`")?;
                    for id in ids_arg(u, "nodeIds")? {
                        live(&o.editor.doc, id)?;
                        let mut bad = Vec::new();
                        ops.extend(commands::style_ops(o, id, &styles, &mut bad)?);
                        ignored.extend(bad.into_iter().map(|b| json!({"nodeId": id, "reason": b})));
                        touched.push(id);
                        count += 1;
                    }
                }
                Ok(((ignored, count), tx(o, ops)?))
            })?;
            app.hub.agent_activity(session, &file, &boards(&d.editor.doc, &touched), &touched, true);
            ok_json(d, json!({"updatedNodes": count, "ignoredStyles": ignored}))
        }
        "set_text_content" => {
            let updates = args["updates"].as_array().cloned().ok_or("`updates` is required")?;
            let ids: Vec<NodeId> =
                updates.iter().map(|u| node_id(u["nodeId"].as_str().unwrap_or_default())).collect::<R<_>>()?;
            write(d, |o| {
                let mut ops = Vec::new();
                for (u, id) in updates.iter().zip(&ids) {
                    let n = live(&o.editor.doc, *id)?;
                    if n.kind != NodeKind::Text {
                        return Err(format!("{} ({id}) is not a text layer", n.name));
                    }
                    let t = u["textContent"].as_str().ok_or("`textContent` must be a string")?;
                    ops.push(Op::Set { id: *id, prop: "text".into(), value: json!(t) });
                }
                Ok(((), tx(o, ops)?))
            })?;
            app.hub.agent_activity(session, &file, &boards(&d.editor.doc, &ids), &ids, true);
            ok_json(d, json!({"updated": ids.len()}))
        }
        "rename_nodes" => {
            let updates = args["updates"].as_array().cloned().ok_or("`updates` is required")?;
            write(d, |o| {
                let mut ops = Vec::new();
                for u in &updates {
                    let id = node_id(u["nodeId"].as_str().unwrap_or_default())?;
                    live(&o.editor.doc, id)?;
                    let name: String = u["name"].as_str().unwrap_or_default().trim().chars().take(100).collect();
                    if name.is_empty() {
                        return Err("names can't be empty".into());
                    }
                    ops.push(Op::Set { id, prop: "name".into(), value: json!(name) });
                }
                Ok(((), tx(o, ops)?))
            })?;
            ok_json(d, json!({"renamed": updates.len()}))
        }
        "duplicate_nodes" => {
            let items = args["nodes"].as_array().cloned().ok_or("`nodes` is required")?;
            let (result, _) = write(d, |o| duplicate_nodes(o, &items))?;
            let new: Vec<NodeId> =
                result["nodes"].as_array().unwrap().iter().filter_map(|v| v["newId"].as_str()?.parse().ok()).collect();
            app.hub.agent_activity(session, &file, &boards(&d.editor.doc, &new), &new, true);
            ok_json(d, result)
        }
        "move_nodes" => {
            let moves = args["moves"].as_array().cloned().ok_or("`moves` is required")?;
            let (result, _) = write(d, |o| move_nodes(o, &moves))?;
            ok_json(d, result)
        }
        "delete_nodes" => {
            let ids = ids_arg(args, "nodeIds")?;
            let b = boards(&d.editor.doc, &ids);
            write(d, |o| {
                for id in &ids {
                    live(&o.editor.doc, *id)?;
                }
                let doc = &o.editor.doc;
                let ops: Vec<Op> = ids
                    .iter()
                    .filter(|id| !doc.ancestors(**id).iter().any(|a| ids.contains(a)))
                    .map(|id| Op::Delete { id: *id })
                    .collect();
                o.editor.selection.retain(|s| !ids.contains(s));
                Ok(((), tx(o, ops)?))
            })?;
            app.hub.agent_activity(session, &file, &b, &[], true);
            ok_json(d, json!({"deleted": ids.len()}))
        }
        "finish_working_on_nodes" => {
            let doc = &d.editor.doc;
            match args.get("nodeIds").and_then(Value::as_array) {
                Some(_) => {
                    let ids = ids_arg(args, "nodeIds")?;
                    app.hub.agent_finish(session, &file, Some(&boards(doc, &ids)));
                }
                None => app.hub.agent_finish(session, &file, None),
            }
            ok_json(d, json!({"finished": true}))
        }
        other => Err(format!("unknown tool `{other}`")),
    }
}

// ---- read tools ---------------------------------------------------------------------------------

fn basic_info(d: &OpenDoc, args: &Value) -> R<Content> {
    let page = page_for(d, args)?;
    let doc = &d.editor.doc;
    let boards: Vec<NodeId> = doc.children(Parent::Page(page)).filter(|c| doc.node(*c).is_some()).collect();
    let artboards: Vec<Value> = boards.iter().take(100).map(|id| summary(doc, *id)).collect();
    let mut fonts = BTreeSet::new();
    for p in &doc.pages {
        for id in doc.descendants(Parent::Page(p.id)) {
            if let Some(f) = doc.node(id).and_then(|n| n.style.font_family.as_ref()) {
                fonts.insert(f.split(',').next().unwrap_or("").trim().trim_matches(['"', '\'']).to_string());
            }
        }
    }
    let pg = doc.page(page).unwrap();
    let mut out = json!({
        "fileId": doc.id, "fileName": doc.name, "filePath": d.path,
        "pageId": page, "pageName": pg.name,
        "nodeCount": doc.descendants(Parent::Page(page)).len(),
        "artboardCount": boards.len(),
        "artboards": artboards,
        "pages": doc.pages.iter().map(|p| json!({"type": if p.divider { "divider" } else { "page" }, "id": p.id, "name": p.name, "icon": p.icon, "isActive": p.id == page})).collect::<Vec<_>>(),
        "fontFamilies": fonts,
        "tokens": {"count": doc.tokens.len(), "items": doc.tokens.values().take(200).map(|t| json!({"name": t.name, "value": t.value})).collect::<Vec<_>>()},
    });
    if boards.len() > 100 {
        out["truncated"] = json!(true);
        out["truncatedMessage"] =
            json!(format!("showing 100 of {} artboards; use get_children on the page for the rest", boards.len()));
    }
    ok_json(d, out)
}

fn tree_lines(doc: &Document, id: NodeId, level: usize, depth: usize, out: &mut String) {
    let Some(n) = doc.node(id) else { return };
    let r = doc.measured.get(&id);
    let size = r.map(|r| format!("{}x{}", round(r.w), round(r.h))).unwrap_or_else(|| "?x?".into());
    let mut line = format!("{}{} \"{}\" ({id}) {size}", "  ".repeat(level), kind_name(n.kind), n.name);
    if let Some(t) = &n.text {
        let short: String = t.chars().take(60).collect();
        line.push_str(&format!(": {:?}", short));
    }
    if n.hidden {
        line.push_str(" [hidden]");
    }
    out.push_str(&line);
    out.push('\n');
    let kids: Vec<NodeId> = doc.children(Parent::Node(id)).filter(|c| doc.node(*c).is_some()).collect();
    if level + 1 >= depth {
        if !kids.is_empty() {
            out.push_str(&format!("{}… {} children\n", "  ".repeat(level + 1), kids.len()));
        }
        return;
    }
    for k in kids {
        tree_lines(doc, k, level + 1, depth, out);
    }
}

fn color_eq(a: &str, b: &Color, tokens: &spark_core::doc::Document) -> bool {
    let resolve_color = |v: &str| -> Option<Color> {
        let v = match var_name(v) {
            Some(name) => resolve(&tokens.tokens, name)?,
            None => v.to_string(),
        };
        v.parse().ok()
    };
    match (resolve_color(a), b.to_hex()) {
        (Some(c), Some(hex)) => c.to_hex().as_deref() == Some(hex.as_str()) && (c.alpha() - b.alpha()).abs() < 0.01,
        _ => false,
    }
}

fn find_nodes(d: &OpenDoc, args: &Value) -> R<Content> {
    let doc = &d.editor.doc;
    let limit = args["limit"].as_u64().unwrap_or(100) as usize;
    let scope: Vec<NodeId> = if let Some(n) = s(args, "nodeId") {
        let id = node_id(n)?;
        live(doc, id)?;
        std::iter::once(id).chain(doc.descendants(Parent::Node(id))).collect()
    } else if s(args, "pageId").is_some() {
        doc.descendants(Parent::Page(page_for(d, args)?))
    } else {
        doc.pages.iter().flat_map(|p| doc.descendants(Parent::Page(p.id))).collect()
    };
    let text_q = s(args, "textValue");
    let filters: Vec<(Option<String>, Option<String>)> = args["filters"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|f| (f["styleName"].as_str().map(str::to_string), f["styleValue"].as_str().map(str::to_string)))
                .collect()
        })
        .unwrap_or_default();
    if text_q.is_none() && filters.is_empty() {
        return Err("give textValue and/or filters".into());
    }
    let mut found = Vec::new();
    for id in scope {
        let n = doc.node(id).unwrap();
        let mut matched = Vec::new();
        if let Some(q) = text_q {
            match &n.text {
                Some(t) if glob(q, t) => matched.push(json!({"textValue": t})),
                _ => continue,
            }
        }
        let css = node_css(doc, id);
        let mut all = true;
        for (name, value) in &filters {
            let hit = css.iter().find(|(k, v)| {
                let name_ok = name.as_deref().is_none_or(|p| glob(p, k) || glob(p, &camel(k)));
                let value_ok = value.as_deref().is_none_or(|q| {
                    glob(q, v)
                        || (!q.contains('*')
                            && q.parse::<Color>().is_ok_and(|qc| {
                                color_eq(v, &qc, doc) || split_colors(v).iter().any(|c| color_eq(c, &qc, doc))
                            }))
                });
                name_ok && value_ok
            });
            match hit {
                Some((k, v)) => matched.push(json!({"styleName": camel(k), "styleValue": v})),
                None => {
                    all = false;
                    break;
                }
            }
        }
        if all {
            found.push(json!({"id": id, "name": n.name, "component": kind_name(n.kind), "matched": matched}));
            if found.len() >= limit {
                break;
            }
        }
    }
    ok_json(d, json!({"nodes": found, "count": found.len()}))
}

/// Color-looking words inside a CSS value (borders, shadows, gradients).
fn split_colors(v: &str) -> Vec<String> {
    spark_core::css::split_top(v, ' ').into_iter().map(|w| w.trim_end_matches(',').to_string()).collect()
}

fn fill_image(d: &OpenDoc, args: &Value) -> R<Content> {
    let id = node_id(req(args, "nodeId")?)?;
    let doc = &d.editor.doc;
    let n = live(doc, id)?;
    if n.image.as_ref().is_some_and(|i| ImageStatus::of(i) != ImageStatus::Ready) {
        return ok_json(
            d,
            json!({"message": "this layer's image is still being generated or requested; try again later"}),
        );
    }
    let Some(hash) = n.style.fills.iter().find_map(|f| match f {
        Fill::Image { asset, .. } => Some(asset.clone()),
        _ => None,
    }) else {
        return ok_json(d, json!({"message": "this layer has no image fill"}));
    };
    let a = doc.assets.get(&hash).ok_or("the image's data is missing from the file")?;
    if a.mime == "image/svg+xml" {
        return ok_json(d, json!({"message": "the image is an SVG", "svg": String::from_utf8_lossy(&a.bytes)}));
    }
    let img = image::load_from_memory(&a.bytes).map_err(|e| format!("can't decode the image: {e}"))?;
    let (w, h) = (img.width(), img.height());
    let small = if w.max(h) > 1024 { img.resize(1024, 1024, image::imageops::FilterType::Triangle) } else { img };
    let mut jpg = Vec::new();
    small
        .to_rgb8()
        .write_to(&mut std::io::Cursor::new(&mut jpg), image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;
    Ok(vec![
        header(d),
        text(json!({"width": w, "height": h, "mimeType": a.mime, "assetHash": hash}).to_string()),
        json!({"type": "image", "data": base64::engine::general_purpose::STANDARD.encode(jpg), "mimeType": "image/jpeg"}),
    ])
}

async fn screenshot(app: &AppState, session: &str, args: &Value) -> R<Content> {
    let doc = doc_for(app, args)?;
    let id = node_id(req(args, "nodeId")?)?;
    let scale = args["scale"].as_f64().unwrap_or(1.0).clamp(1.0, 2.0);
    let (file, head) = {
        let d = doc.lock().unwrap();
        live(&d.editor.doc, id)?;
        (d.id().to_string(), header(&d))
    };
    let png = app.hub.render(&file, id, scale).await?;
    app.hub.agent_activity(session, &file, &[], &[id], false);
    Ok(vec![
        head,
        json!({"type": "image", "data": base64::engine::general_purpose::STANDARD.encode(png), "mimeType": "image/png"}),
    ])
}

// ---- fonts ---------------------------------------------------------------------------------------

async fn font_info(app: &AppState, args: &Value) -> R<Content> {
    let families: Vec<String> = args["familyNames"]
        .as_array()
        .ok_or("`familyNames` is required")?
        .iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect();
    let mut out = Map::new();
    let mut google: Option<Option<Value>> = None;
    for fam in &families {
        let key = fam.trim().to_lowercase();
        let mut styles: Vec<Value> = Vec::new();
        let mut sources = Vec::new();
        if key == "geist" || key == "geist mono" {
            sources.push("bundled");
            styles.push(json!({"weight": "100-900", "style": "normal", "isItalic": false, "axes": {"wght": {"min": 100, "max": 900}}}));
        }
        if let Some(local) = crate::fonts::local_fonts().get(&key) {
            sources.push("local");
            styles.extend(local.iter().cloned());
        }
        if sources.is_empty() {
            let g = match &google {
                Some(g) => g.clone(),
                None => {
                    let g = crate::fonts::google_fonts(app).await;
                    google = Some(g.clone());
                    g
                }
            };
            if let Some(f) = g
                .as_ref()
                .and_then(|g| g.as_array())
                .and_then(|l| l.iter().find(|f| f["family"].as_str().is_some_and(|n| n.to_lowercase() == key)))
            {
                sources.push("google");
                for k in f["fonts"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                    let italic = k.ends_with('i');
                    styles.push(json!({"weight": k.trim_end_matches('i').parse::<u32>().unwrap_or(400), "style": if italic { "italic" } else { "normal" }, "isItalic": italic}));
                }
                if let Some(axes) = f["axes"].as_array().filter(|a| !a.is_empty()) {
                    styles.push(json!({"axes": axes}));
                }
            }
        }
        out.insert(fam.clone(), json!({"available": !sources.is_empty(), "sources": sources, "styles": styles}));
    }
    let mut payload = json!({"fontsPerFamily": out});
    if google == Some(None) {
        payload["note"] =
            json!("Google Fonts metadata could not be loaded (offline?); only local and bundled fonts were checked.");
    }
    Ok(vec![text(serde_json::to_string_pretty(&payload).unwrap())])
}

// ---- tokens --------------------------------------------------------------------------------------

fn get_tokens(d: &OpenDoc, args: &Value) -> R<Content> {
    let doc = &d.editor.doc;
    let types: Vec<String> = args["types"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let pattern = s(args, "namePattern");
    let selected: std::collections::BTreeMap<String, Token> = doc
        .tokens
        .iter()
        .filter(|(n, _)| types.is_empty() || token_type(n).is_some_and(|t| types.iter().any(|x| x == t)))
        .filter(|(n, _)| pattern.is_none_or(|p| glob(p, n)))
        .map(|(n, t)| (n.clone(), t.clone()))
        .collect();
    match args["format"].as_str().unwrap_or("json") {
        "css" => Ok(vec![header(d), text(to_css(&selected, false))]),
        "tailwind" => Ok(vec![header(d), text(to_css(&selected, true))]),
        _ => ok_json(
            d,
            json!({"tokens": selected.values().map(|t| json!({"name": t.name, "type": token_type(&t.name), "value": t.value, "resolvedValue": resolve(&doc.tokens, &t.name), "description": t.description})).collect::<Vec<_>>()}),
        ),
    }
}

fn value_str(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn create_tokens(d: &mut OpenDoc, args: &Value) -> R<Content> {
    let items = args["tokens"].as_array().cloned().ok_or("`tokens` is required")?;
    let (results, _) = write(d, |o| {
        let mut results = Vec::new();
        let mut ops = Vec::new();
        for t in &items {
            let name = t["name"].as_str().unwrap_or_default().to_string();
            let problem = if let Err(e) = validate_name(&name) {
                Some(e)
            } else if o.editor.doc.tokens.contains_key(&name)
                || ops.iter().any(|op| matches!(op, Op::SetToken { name: n, .. } if *n == name))
            {
                Some(format!("{name} already exists; use set_tokens to change it"))
            } else if let Some(ty) = t["type"].as_str().filter(|ty| token_type(&name) != Some(*ty)) {
                Some(format!(
                    "{name} is not a {ty} token (its namespace makes it {})",
                    token_type(&name).unwrap_or("unknown")
                ))
            } else {
                value_str(&t["value"]).is_none().then(|| format!("{name} needs a value"))
            };
            match problem {
                Some(message) => results.push(json!({"name": name, "result": "error", "message": message})),
                None => {
                    let token = Token {
                        name: name.clone(),
                        value: value_str(&t["value"]).unwrap(),
                        description: t["description"].as_str().map(str::to_string),
                    };
                    ops.push(Op::SetToken { name: name.clone(), token: Some(token) });
                    results.push(json!({"name": name, "result": "created"}));
                }
            }
        }
        Ok((results, tx(o, ops)?))
    })?;
    ok_json(d, json!(results))
}

fn set_tokens(d: &mut OpenDoc, args: &Value) -> R<Content> {
    let items = args["tokens"].as_array().cloned().ok_or("`tokens` is required")?;
    let (results, _) = write(d, |o| {
        let mut results = Vec::new();
        for t in &items {
            let name = t["name"].as_str().unwrap_or_default().to_string();
            let Some(old) = o.editor.doc.tokens.get(&name).cloned() else {
                results.push(json!({"name": name, "result": "error", "message": format!("{name} not found")}));
                continue;
            };
            let ops = if t["delete"].as_bool().unwrap_or(false) {
                vec![Op::SetToken { name: name.clone(), token: None }]
            } else {
                let new_name = t["newName"].as_str().map(str::to_string).unwrap_or(name.clone());
                let token = Token {
                    name: new_name.clone(),
                    value: value_str(&t["value"]).unwrap_or(old.value),
                    description: t
                        .get("description")
                        .map(|d| d.as_str().unwrap_or("").to_string())
                        .filter(|d| !d.is_empty())
                        .or(old.description),
                };
                let mut ops = Vec::new();
                if new_name != name {
                    ops.push(Op::SetToken { name: name.clone(), token: None });
                }
                ops.push(Op::SetToken { name: new_name, token: Some(token) });
                ops
            };
            match tx(o, ops) {
                Ok(_) => results.push(json!({"name": name, "result": if t["delete"].as_bool().unwrap_or(false) { "deleted" } else { "updated" }})),
                Err(e) => results.push(json!({"name": name, "result": "error", "message": e})),
            }
        }
        Ok((results, Vec::new()))
    })?;
    // Changes were applied inside the transaction; resend the whole token list.
    d.broadcast(&json!({"t": "patch", "file": d.id(), "nodes": {}, "tokens": d.editor.doc.tokens.values().collect::<Vec<_>>(), "canUndo": d.editor.can_undo(), "canRedo": d.editor.can_redo()}));
    d.save(&[]);
    ok_json(d, json!(results))
}

// ---- pages and files ------------------------------------------------------------------------------

fn arrange_pages(d: &mut OpenDoc, args: &Value) -> R<Content> {
    let order = args["order"].as_array().cloned().ok_or("`order` is required")?;
    let remove: Vec<PageId> =
        args["removeDividerIds"].as_array().into_iter().flatten().filter_map(|v| v.as_str()?.parse().ok()).collect();
    write(d, |o| {
        let doc = &o.editor.doc;
        for r in &remove {
            if !doc.page(*r).is_some_and(|p| p.divider) {
                return Err(format!("{r} is not a divider"));
            }
        }
        let keep: Vec<PageId> = doc.pages.iter().map(|p| p.id).filter(|p| !remove.contains(p)).collect();
        let listed: Vec<PageId> = order.iter().filter_map(|v| v.as_str()?.parse().ok()).collect();
        let mut sorted_keep = keep.clone();
        sorted_keep.sort();
        let mut sorted_listed = listed.clone();
        sorted_listed.sort();
        if sorted_keep != sorted_listed {
            return Err("order must list every page and divider exactly once (minus removeDividerIds); read the page list again".into());
        }
        let mut ops: Vec<Op> = remove.iter().map(|id| Op::DeletePage { id: *id }).collect();
        let mut done = tx(o, std::mem::take(&mut ops))?;
        for (i, entry) in order.iter().enumerate() {
            let id = match entry.as_str() {
                Some(p) => p.parse().map_err(|_| format!("bad page id {p}"))?,
                None => {
                    let id = o.editor.doc.alloc_page();
                    done.extend(tx(
                        o,
                        vec![Op::InsertPage {
                            page: Page { id, name: String::new(), icon: None, canvas: None, divider: true },
                            index: i,
                        }],
                    )?);
                    continue;
                }
            };
            done.extend(tx(o, vec![Op::MovePage { id, index: i }])?);
        }
        Ok(((), done))
    })?;
    ok_json(
        d,
        json!({"pages": d.editor.doc.pages.iter().map(|p| json!({"type": if p.divider { "divider" } else { "page" }, "id": p.id, "name": p.name})).collect::<Vec<_>>()}),
    )
}

fn list_resources(app: &AppState, args: &Value) -> R<Content> {
    let limit = args["limit"].as_u64().unwrap_or(50) as usize;
    let viewed = app.hub.viewed().map(|d| d.lock().unwrap().id().to_string());
    let mut out: Vec<Value> = Vec::new();
    for d in app.hub.open_docs() {
        let d = d.lock().unwrap();
        out.push(json!({"id": d.id(), "name": d.editor.doc.name, "path": d.path, "type": "file", "open": true, "active": viewed.as_deref() == Some(d.id())}));
    }
    for f in app.hub.config.recent() {
        if !out.iter().any(|o| o["id"] == f.id) {
            out.push(json!({"id": f.id, "name": f.name, "path": f.path, "type": "file", "open": false, "openedAt": f.opened_at}));
        }
    }
    let total = out.len();
    out.truncate(limit);
    let mut payload = json!({"resources": out, "count": total});
    if total > limit {
        payload["truncated"] = json!(true);
    }
    Ok(vec![text(serde_json::to_string_pretty(&payload).unwrap())])
}

fn create_file(app: &AppState, args: &Value) -> R<Content> {
    let name = s(args, "name").unwrap_or("Untitled").to_string();
    let doc = match s(args, "cloneFileId") {
        None => app.hub.create(&name)?,
        Some(src) => {
            let src = app.hub.open_ref(src)?;
            let path = {
                let mut d = src.lock().unwrap();
                d.save(&[]);
                d.path.clone()
            };
            let (_, mut copy) = spark_core::store::Store::open(&path).map_err(|e| e.to_string())?;
            copy.id = spark_core::id::new_file_id();
            copy.name = name.clone();
            let new_path = app.hub.config.new_file_path(&name);
            std::fs::create_dir_all(&app.hub.config.files_dir).map_err(|e| e.to_string())?;
            spark_core::store::Store::create(&new_path, &mut copy).map_err(|e| e.to_string())?;
            app.hub.open_path(&new_path)?
        }
    };
    let d = doc.lock().unwrap();
    Ok(vec![text(
        serde_json::to_string_pretty(&json!({"fileId": d.id(), "name": d.editor.doc.name, "path": d.path})).unwrap(),
    )])
}

// ---- write tools ----------------------------------------------------------------------------------

fn create_artboard(o: &mut OpenDoc, args: &Value) -> R<(NodeId, Vec<Op>)> {
    let page = page_for(o, args)?;
    let name = req(args, "name")?.to_string();
    let doc = &mut o.editor.doc;
    let mut node = Node::new(doc.alloc_node(), NodeKind::Frame);
    node.name = name;
    let st = &mut node.style;
    st.width = Some(Size::Fixed(Len::Px(1440.0)));
    st.height = Some(Size::Hug);
    st.display = Some("flex".into());
    st.flex_direction = Some("column".into());
    st.overflow = Some("clip".into());
    st.fills = vec![Fill::Solid { color: Color::WHITE, hidden: false }];
    if let Some(styles) = args["styles"].as_object() {
        for (k, v) in styles {
            let v = match v {
                Value::Number(n)
                    if !matches!(k.as_str(), "opacity" | "zIndex" | "flexGrow" | "fontWeight" | "lineHeight") =>
                {
                    format!("{n}px")
                }
                Value::Number(n) => n.to_string(),
                Value::String(s) => s.clone(),
                _ => continue,
            };
            apply_decl(st, k, &v).map_err(|e| format!("styles: {e}"))?;
        }
    }
    // Auto-place to the right of everything on the page, top-aligned.
    let rects: Vec<Rect> = doc.children(Parent::Page(page)).filter_map(|id| rect_of(doc, id)).collect();
    let (x, y) = match rects.iter().copied().reduce(|a, b| Rect {
        x: a.x.min(b.x),
        y: a.y.min(b.y),
        w: a.right().max(b.right()) - a.x.min(b.x),
        h: 0.0,
    }) {
        Some(u) => (round(u.x + u.w + 80.0), round(u.y)),
        None => (0.0, 0.0),
    };
    st.position = Some("absolute".into());
    st.left = Some(Len::Px(x));
    st.top = Some(Len::Px(y));
    let id = node.id;
    let key = doc.key_at(Parent::Page(page), None, None);
    Ok((id, tx(o, vec![Op::Insert { node, parent: Parent::Page(page), key }])?))
}

fn target_of(o: &OpenDoc, raw: &str) -> R<Parent> {
    if raw == "root" {
        return Ok(Parent::Page(page_for(o, &json!({}))?));
    }
    if raw.starts_with("p-") {
        let p: PageId = raw.parse().map_err(|_| format!("`{raw}` is not a page id"))?;
        return match o.editor.doc.page(p) {
            Some(pg) if !pg.divider => Ok(Parent::Page(p)),
            _ => Err(format!("page {raw} not found")),
        };
    }
    let id = node_id(raw)?;
    live(&o.editor.doc, id)?;
    Ok(Parent::Node(id))
}

async fn write_html(app: &AppState, session: &str, args: &Value) -> R<Content> {
    let doc = doc_for(app, args)?;
    let html = req(args, "html")?.to_string();
    let target = req(args, "targetNodeId")?.to_string();
    let replace = args["mode"] == "replace";
    let settings = Settings::load(&app.hub.config);
    let loaded = images::prefetch(&images::image_urls(&html), &settings).await;
    let mut d = doc.lock().unwrap();
    let file = d.id().to_string();
    let jobs = crate::providers::JobQueue::default();
    let mut charged = 0.0;
    let written = write(&mut d, |o| {
        let t = target_of(o, &target)?;
        let (parent, index) = match (replace, t) {
            (true, Parent::Node(id)) => {
                let parent = o.editor.doc.parent(id).unwrap();
                (parent, o.editor.doc.children(parent).position(|c| c == id))
            }
            (true, Parent::Page(_)) => return Err("replace needs a layer id, not a page".into()),
            (false, p) => (p, None),
        };
        let mut resolve = crate::providers::resolver(&loaded, &jobs, &settings);
        let imp = spark_core::html::import(&mut o.editor.doc, &html, parent, index, &mut resolve);
        drop(resolve);
        if imp.roots.is_empty() {
            return Err(if imp.notes.is_empty() { "the HTML produced no layers".into() } else { imp.notes.join("; ") });
        }
        // The cost guard rejects the whole call before anything is written.
        charged = crate::generate::charge(&app.hub.config, &jobs.0.lock().unwrap())?;
        let mut ops = imp.ops;
        if matches!(parent, Parent::Page(_)) {
            edit::position_roots(&o.editor.doc, &mut ops, &imp.roots, &edit::Placement { parent, at: None }, &[]);
        }
        let mut done = tx(o, ops)?;
        if let (true, Parent::Node(old)) = (replace, t) {
            done.extend(tx(o, vec![Op::Delete { id: old }])?);
        }
        let b = boards(&o.editor.doc, &imp.roots);
        Ok(((imp.roots, imp.notes, b), done))
    });
    if written.is_err() {
        crate::generate::refund(&app.hub.config, charged);
    }
    let ((roots, notes, b), _) = written?;
    let created: Vec<Value> = roots
        .iter()
        .filter_map(|id| {
            d.editor.doc.node(*id).map(|n| json!({"id": id, "name": n.name, "component": kind_name(n.kind)}))
        })
        .collect();
    let mut payload = json!({"created": created});
    if !notes.is_empty() {
        payload["notes"] = json!(notes);
    }
    let out = ok_json(&d, payload);
    drop(d);
    app.hub.agent_activity(session, &file, &b, &roots, true);
    crate::providers::start(app.hub.clone(), file, jobs);
    out
}

async fn insert_image(app: &AppState, session: &str, args: &Value) -> R<Content> {
    let doc = doc_for(app, args)?;
    let settings = Settings::load(&app.hub.config);
    let bytes = if let Some(p) = s(args, "path") {
        crate::settings::read_granted_image(&settings, std::path::Path::new(p))?
    } else if let Some(u) = s(args, "url") {
        images::fetch_url(u).await?
    } else if let Some(b) = s(args, "base64") {
        let b = b.split_once(";base64,").map(|(_, d)| d).unwrap_or(b);
        base64::engine::general_purpose::STANDARD.decode(b.trim()).map_err(|e| format!("bad base64: {e}"))?
    } else {
        return Err("give one of path, url or base64".into());
    };
    let mut d = doc.lock().unwrap();
    let file = d.id().to_string();
    let asset = crate::add_asset(&mut d, &bytes)?;
    let (iw, ih) = (asset["width"].as_f64().unwrap_or(512.0), asset["height"].as_f64().unwrap_or(512.0));
    let (w, h) = match (args["width"].as_f64(), args["height"].as_f64()) {
        (Some(w), Some(h)) => (w, h),
        (Some(w), None) => (w, w * ih / iw),
        (None, Some(h)) => (h * iw / ih, h),
        (None, None) => {
            let k = (1024.0 / iw.max(ih)).min(1.0);
            (iw * k, ih * k)
        }
    };
    let hash = asset["hash"].as_str().unwrap().to_string();
    let (id, _) = write(&mut d, |o| {
        let parent = match s(args, "parentId") {
            Some(p) => target_of(o, p)?,
            None => Parent::Page(page_for(o, args)?),
        };
        let (x, y) = match (args["x"].as_f64(), args["y"].as_f64(), parent) {
            (Some(x), Some(y), Parent::Node(p)) => {
                let (ox, oy) = edit::origin_of(&o.editor.doc, Parent::Node(p));
                (ox + x, oy + y)
            }
            (Some(x), Some(y), _) => (x, y),
            (_, _, Parent::Page(page)) => edit::free_spot(&o.editor.doc, page),
            (_, _, Parent::Node(p)) => edit::origin_of(&o.editor.doc, Parent::Node(p)),
        };
        let mut node = commands::new_node(o, NodeKind::Rect, parent, x, y, w, h);
        node.name = s(args, "name").unwrap_or("Image").to_string();
        node.style.fills = vec![Fill::Image { asset: hash.clone(), fit: Fit::Cover, hidden: false }];
        let id = node.id;
        let key = o.editor.doc.key_at(parent, None, None);
        Ok((id, tx(o, vec![Op::Insert { node, parent, key }])?))
    })?;
    let b = boards(&d.editor.doc, &[id]);
    let out = ok_json(&d, json!({"id": id, "width": round(w), "height": round(h)}));
    drop(d);
    app.hub.agent_activity(session, &file, &b, &[id], true);
    out
}

async fn export(app: &AppState, args: &Value) -> R<Content> {
    let doc = doc_for(app, args)?;
    let dir = s(args, "dir").map(std::path::PathBuf::from).unwrap_or_else(crate::export::downloads_dir);
    let jobs: Vec<(NodeId, Value)> = {
        let d = doc.lock().unwrap();
        let ed = &d.editor.doc;
        match &args["nodes"] {
            Value::String(s) if s == "nodes-with-exports-only" => {
                let page = page_for(&d, args)?;
                ed.descendants(Parent::Page(page))
                    .into_iter()
                    .filter_map(|id| {
                        let n = ed.node(id)?;
                        (!n.exports.is_empty()).then(|| (id, serde_json::to_value(&n.exports).unwrap()))
                    })
                    .collect()
            }
            Value::Object(map) => map
                .iter()
                .map(|(k, v)| {
                    let id = node_id(k)?;
                    live(ed, id)?;
                    Ok((id, v.clone()))
                })
                .collect::<R<_>>()?,
            _ => {
                return Err(
                    "`nodes` must be an object of node id → export settings, or \"nodes-with-exports-only\"".into()
                );
            }
        }
    };
    if jobs.is_empty() {
        return Err("no layers on this page have export presets".into());
    }
    let mut files = Vec::new();
    for (id, presets) in jobs {
        let presets = crate::export::presets(&presets)?;
        for path in crate::export::export_presets(Some(&app.hub), &doc, id, &presets, &dir).await? {
            files.push(json!({"nodeId": id, "path": path}));
        }
    }
    let d = doc.lock().unwrap();
    ok_json(&d, json!({"files": files}))
}

async fn fulfill(app: &AppState, session: &str, args: &Value) -> R<Content> {
    let doc = doc_for(app, args)?;
    let request = req(args, "requestId")?.to_string();
    let settings = Settings::load(&app.hub.config);
    let bytes = if let Some(p) = s(args, "path") {
        crate::settings::read_granted_image(&settings, std::path::Path::new(p))?
    } else if let Some(u) = s(args, "url") {
        images::fetch_url(u).await?
    } else if let Some(b) = s(args, "base64") {
        let b = b.split_once(";base64,").map(|(_, d)| d).unwrap_or(b);
        base64::engine::general_purpose::STANDARD.decode(b.trim()).map_err(|e| format!("bad base64: {e}"))?
    } else {
        return Err("give one of path, url or base64".into());
    };
    spark_core::asset::image_mime(&bytes).ok_or("that isn't a supported image (png, jpeg, gif, webp, avif or svg)")?;
    let file = doc.lock().unwrap().id().to_string();
    {
        let d = doc.lock().unwrap();
        let id = crate::generate::node_for_request(&d.editor.doc, &request)
            .ok_or_else(|| format!("no image request {request}; call list_image_requests"))?;
        if d.editor
            .doc
            .node(id)
            .and_then(|n| n.image.as_ref())
            .is_some_and(|i| ImageStatus::of(i) == ImageStatus::Ready)
        {
            return Err(format!("image request {request} is already fulfilled"));
        }
    }
    let id = crate::generate::finish(&app.hub, &file, &request, Ok(bytes))?;
    let d = doc.lock().unwrap();
    let b = boards(&d.editor.doc, &[id]);
    let out = ok_json(&d, json!({"nodeId": id, "status": "ready"}));
    drop(d);
    app.hub.agent_activity(session, &file, &b, &[id], true);
    out
}

fn duplicate_nodes(o: &mut OpenDoc, items: &[Value]) -> R<(Value, Vec<Op>)> {
    let mut results = Vec::new();
    let mut map = Map::new();
    let mut done = Vec::new();
    for item in items {
        let src = node_id(item["id"].as_str().unwrap_or_default())?;
        live(&o.editor.doc, src)?;
        let (new_id, ops, pairs) = match item["parentId"].as_str() {
            Some(p) => {
                let parent = target_of(o, p)?;
                let key = o.editor.doc.key_at(parent, None, None);
                let (nid, ops, pairs) = edit::clone_with_map(&mut o.editor.doc, src, parent, key);
                (nid, ops, pairs)
            }
            None => {
                let (ids, ops, pairs) = edit::duplicate(o, &[src], true)?;
                let Some(&nid) = ids.first() else { return Err(format!("{src} is locked")) };
                (nid, ops, pairs)
            }
        };
        done.extend(tx(o, ops)?);
        for (a, b) in pairs {
            map.insert(a.to_string(), json!(b));
        }
        results.push(json!({"sourceId": src, "newId": new_id}));
    }
    Ok((json!({"nodes": results, "descendantIdMap": map}), done))
}

fn move_nodes(o: &mut OpenDoc, moves: &[Value]) -> R<(Value, Vec<Op>)> {
    let mut done = Vec::new();
    let mut results = Vec::new();
    let mut affected = BTreeSet::new();
    for m in moves {
        let id = node_id(m["nodeId"].as_str().unwrap_or_default())?;
        live(&o.editor.doc, id)?;
        let doc = &o.editor.doc;
        let (parent, index) = if let Some(b) = m["before"].as_str().or(m["after"].as_str()) {
            let sib = node_id(b)?;
            live(doc, sib)?;
            if sib == id {
                return Err(format!("{id} can't move before or after itself"));
            }
            let parent = doc.parent(sib).unwrap();
            let i = doc.children(parent).filter(|c| *c != id).position(|c| c == sib).unwrap();
            (parent, if m["after"].is_string() { i + 1 } else { i })
        } else if let Some(p) = m["parentId"].as_str() {
            let parent = target_of(o, p)?;
            let len = o.editor.doc.children(parent).filter(|c| *c != id).count();
            (parent, m["index"].as_u64().map(|i| i as usize).unwrap_or(len).min(len))
        } else {
            return Err("each move needs before, after or parentId".into());
        };
        let doc = &o.editor.doc;
        if let Some(old) = doc.parent(id) {
            affected.insert(old.to_string());
        }
        affected.insert(parent.to_string());
        let world: Vec<(NodeId, Rect)> = doc.measured.get(&id).map(|r| (id, *r)).into_iter().collect();
        let ops = if doc.parent(id) == Some(parent) {
            vec![Op::Move { id, parent, key: doc.key_at(parent, Some(index), Some(id)) }]
        } else {
            edit::reparent_ops(doc, &[id], parent, Some(index), &world)
        };
        done.extend(tx(o, ops)?);
        results.push(json!({"nodeId": id, "parentId": parent.to_string(), "index": index}));
    }
    let doc = &o.editor.doc;
    let lists: Map<String, Value> = affected
        .into_iter()
        .filter_map(|p| {
            let parent: Parent = p.parse().ok()?;
            Some((p, json!(doc.children(parent).collect::<Vec<_>>())))
        })
        .collect();
    Ok((json!({"moves": results, "affectedParents": lists}), done))
}
