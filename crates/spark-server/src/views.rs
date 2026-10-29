//! What the UI receives: node views with compiled CSS, snapshots and patches.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};
use spark_core::{Document, NodeId, Op, Parent, codegen::node_css, css::camel};

use crate::hub::OpenDoc;

/// CSS for a node as a camelCase map, ready for a React `style` prop.
pub fn css_map(doc: &Document, id: NodeId) -> Map<String, Value> {
    let Some(node) = doc.node(id) else { return Map::new() };
    let mut map: Map<String, Value> =
        node_css(doc, id).into_iter().map(|(k, v)| (camel(&k), Value::String(v))).collect();
    // On the canvas every frame is a containing block, so children drawn into it later stay inside.
    if node.kind == spark_core::NodeKind::Frame && !map.contains_key("position") {
        map.insert("position".into(), Value::String("relative".into()));
    }
    map
}

pub fn node_view(doc: &Document, id: NodeId) -> Value {
    let (Some(node), Some((parent, key))) = (doc.node(id), doc.tree_entry(id)) else { return Value::Null };
    let mut v = serde_json::to_value(node).unwrap();
    let o = v.as_object_mut().unwrap();
    o.insert("parent".into(), json!(parent.to_string()));
    o.insert("key".into(), json!(key));
    o.insert("css".into(), Value::Object(css_map(doc, id)));
    v
}

fn history_flags(open: &OpenDoc) -> (bool, bool) {
    (open.editor.can_undo(), open.editor.can_redo())
}

/// A snapshot of the whole document, with layout rects so the canvas can place and cull
/// artboards before it has measured anything itself.
pub fn snapshot(open: &mut OpenDoc) -> Value {
    for p in open.editor.doc.pages.clone() {
        for board in open.editor.doc.children(Parent::Page(p.id)).collect::<Vec<_>>() {
            crate::layout::ensure(&mut open.editor.doc, board);
        }
    }
    let doc = &open.editor.doc;
    let mut nodes = Map::new();
    let mut rects = Map::new();
    let round = |v: f64| (v * 100.0).round() / 100.0;
    for p in &doc.pages {
        for id in doc.descendants(Parent::Page(p.id)) {
            nodes.insert(id.to_string(), node_view(doc, id));
            if let Some(r) = doc.measured.get(&id) {
                rects.insert(id.to_string(), json!([round(r.x), round(r.y), round(r.w), round(r.h)]));
            }
        }
    }
    let (can_undo, can_redo) = history_flags(open);
    json!({
        "t": "snapshot",
        "file": open.file_info(),
        "pages": doc.pages,
        "nodes": nodes,
        "rects": rects,
        "tokens": doc.tokens.values().collect::<Vec<_>>(),
        "page": open.editor.page,
        "selection": open.editor.selection,
        "canUndo": can_undo,
        "canRedo": can_redo,
    })
}

/// The changes `ops` made, as replacement node views (null = removed) plus page/token lists.
pub fn patch(open: &OpenDoc, ops: &[Op]) -> Value {
    let doc = &open.editor.doc;
    let mut touched = BTreeSet::new();
    let (mut pages, mut tokens, mut name) = (false, false, false);
    for op in ops {
        match op {
            Op::Insert { node, .. } => {
                touched.insert(node.id);
            }
            Op::Delete { id } | Op::Move { id, .. } => {
                touched.insert(*id);
            }
            Op::Set { id, prop, .. } => {
                touched.insert(*id);
                // A parent's layout changes how its children's sizing compiles.
                if prop.starts_with("style") {
                    touched.extend(doc.children(Parent::Node(*id)));
                }
            }
            Op::InsertPage { .. } | Op::DeletePage { .. } | Op::SetPage { .. } | Op::MovePage { .. } => pages = true,
            Op::SetToken { .. } => tokens = true,
            Op::SetFileName { .. } => name = true,
            Op::AddAsset { .. } => {}
        }
    }
    let nodes: Map<String, Value> = touched.into_iter().map(|id| (id.to_string(), node_view(doc, id))).collect();
    let (can_undo, can_redo) = history_flags(open);
    let mut msg = json!({
        "t": "patch",
        "file": doc.id,
        "nodes": nodes,
        "page": open.editor.page,
        "selection": open.editor.selection,
        "canUndo": can_undo,
        "canRedo": can_redo,
    });
    if pages {
        msg["pages"] = json!(doc.pages);
    }
    if tokens {
        msg["tokens"] = json!(doc.tokens.values().collect::<Vec<_>>());
    }
    if name {
        msg["name"] = json!(doc.name);
    }
    msg
}
