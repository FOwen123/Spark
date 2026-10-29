//! Readable exports of a whole document.

use serde_json::{Value, json};

use crate::{
    doc::{Document, Parent},
    id::NodeId,
};

fn node_json(doc: &Document, id: NodeId) -> Value {
    let mut v = serde_json::to_value(doc.node(id)).unwrap_or(Value::Null);
    let children: Vec<Value> =
        doc.children(Parent::Node(id)).filter(|c| doc.node(*c).is_some()).map(|c| node_json(doc, c)).collect();
    if let Value::Object(o) = &mut v
        && !children.is_empty()
    {
        o.insert("children".into(), Value::Array(children));
    }
    v
}

/// The document as a nested JSON tree, for diffing and inspection (`spark export --json`).
/// Asset bytes are left out; each asset lists its hash, type and size.
pub fn to_json(doc: &Document) -> Value {
    let pages: Vec<Value> = doc
        .pages
        .iter()
        .map(|p| {
            let mut v = serde_json::to_value(p).unwrap();
            if !p.divider {
                let nodes: Vec<Value> = doc
                    .children(Parent::Page(p.id))
                    .filter(|c| doc.node(*c).is_some())
                    .map(|c| node_json(doc, c))
                    .collect();
                v["children"] = Value::Array(nodes);
            }
            v
        })
        .collect();
    json!({
        "id": doc.id,
        "name": doc.name,
        "schema": crate::store::SCHEMA,
        "pages": pages,
        "tokens": doc.tokens.values().collect::<Vec<_>>(),
        "assets": doc.assets.iter().map(|(h, a)| json!({"hash": h, "mime": a.mime, "bytes": a.bytes.len()})).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_nested_tree() {
        let doc = crate::demo::demo_document(40);
        let v = to_json(&doc);
        let boards = v["pages"][0]["children"].as_array().unwrap();
        assert!(!boards.is_empty());
        assert!(boards[0]["children"].as_array().is_some_and(|c| !c.is_empty()));
        assert_eq!(v["name"], "Demo");
    }
}
