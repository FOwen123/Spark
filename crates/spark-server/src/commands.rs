//! UI intents. The UI never edits the document itself: it sends a command, Rust turns it into
//! validated ops, and every subscriber receives the resulting patch.

use serde::Deserialize;
use serde_json::{Value, json};
use spark_core::{
    Node, NodeId, NodeKind, Op, PageId, Parent,
    color::Color,
    css::{apply_decl, remove_decl},
    node::Page,
    style::{Fill, Len, Size, Style},
    tokens::Token,
};

use crate::{
    edit::{self, Gesture, GestureKind, origin_of, round2},
    hub::{ClientId, OpenDoc},
};
use spark_core::codegen::{self, CodeOpts, Format};

pub type CmdResult = Result<(Value, Vec<Op>), String>;

fn parse<T: for<'de> Deserialize<'de>>(cmd: &Value) -> Result<T, String> {
    serde_json::from_value(cmd.clone()).map_err(|e| format!("bad command: {e}"))
}

fn ids(v: &[String]) -> Result<Vec<NodeId>, String> {
    v.iter().map(|s| s.parse().map_err(|e: spark_core::id::IdError| e.to_string())).collect()
}

/// Style edits from CSS declarations, as one `Set` op per changed style field.
pub fn style_ops(
    open: &OpenDoc,
    id: NodeId,
    styles: &serde_json::Map<String, Value>,
    ignored: &mut Vec<String>,
) -> Result<Vec<Op>, String> {
    let node = open.editor.doc.node(id).ok_or(format!("node {id} not found"))?;
    let mut style = node.style.clone();
    for (prop, value) in styles {
        match value {
            Value::Null => remove_decl(&mut style, prop),
            Value::String(s) if s.trim().is_empty() => remove_decl(&mut style, prop),
            Value::Number(n) => {
                let unitless = matches!(
                    prop.as_str(),
                    "opacity"
                        | "flexGrow"
                        | "flex-grow"
                        | "flexShrink"
                        | "flex-shrink"
                        | "zIndex"
                        | "z-index"
                        | "fontWeight"
                        | "font-weight"
                        | "lineHeight"
                        | "line-height"
                );
                let v = if unitless { n.to_string() } else { format!("{n}px") };
                if let Err(e) = apply_decl(&mut style, prop, &v) {
                    ignored.push(e);
                }
            }
            Value::String(s) => {
                if let Err(e) = apply_decl(&mut style, prop, s) {
                    ignored.push(e);
                }
            }
            other => ignored.push(format!("`{prop}`: value must be a string or number, got {other}")),
        }
    }
    Ok(style_diff(id, &node.style, &style))
}

/// One `Set` op per style field that differs between `before` and `after`.
fn style_diff(id: NodeId, before: &Style, after: &Style) -> Vec<Op> {
    let (before, after) = (serde_json::to_value(before).unwrap(), serde_json::to_value(after).unwrap());
    let mut ops = Vec::new();
    for field in spark_core::style::STYLE_FIELDS {
        let (b, a) = (before.get(*field).unwrap_or(&Value::Null), after.get(*field).unwrap_or(&Value::Null));
        if b != a {
            ops.push(Op::Set { id, prop: format!("style.{field}"), value: a.clone() });
        }
    }
    ops
}

/// Swaps every use of color `from` for `to` in a style: fills, gradient stops, border, shadows, text.
fn replace_color(style: &mut Style, from: &Color, to: &Color) {
    let swap = |c: &mut Color| {
        if c == from {
            *c = to.clone();
        }
    };
    for f in &mut style.fills {
        match f {
            Fill::Solid { color, .. } => swap(color),
            Fill::Linear { stops, .. } | Fill::Radial { stops, .. } | Fill::Conic { stops, .. } => {
                stops.iter_mut().for_each(|s| swap(&mut s.color))
            }
            Fill::Image { .. } => {}
        }
    }
    style.border.iter_mut().for_each(|b| swap(&mut b.color));
    style.shadows.iter_mut().for_each(|s| swap(&mut s.color));
    style.color.iter_mut().for_each(swap);
}

#[derive(Deserialize)]
struct Create {
    kind: NodeKind,
    parent: String,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    index: Option<usize>,
}

/// A new node with Spark's default look for its kind, positioned at world (x, y).
pub fn new_node(open: &mut OpenDoc, kind: NodeKind, parent: Parent, x: f64, y: f64, w: f64, h: f64) -> Node {
    let doc = &mut open.editor.doc;
    let mut node = Node::new(doc.alloc_node(), kind);
    let flex = matches!(parent, Parent::Node(p) if doc.node(p).is_some_and(|n| n.style.is_flex()));
    let (ox, oy) = origin_of(doc, parent);
    let s = &mut node.style;
    if !flex {
        s.position = Some("absolute".into());
        s.left = Some(Len::Px(round2(x - ox)));
        s.top = Some(Len::Px(round2(y - oy)));
    }
    let size = |v: f64| Some(Size::Fixed(Len::Px(round2(v))));
    match kind {
        NodeKind::Text => {
            s.width = if w > 4.0 { size(w) } else { Some(Size::Hug) };
            s.font_family = Some("Geist, sans-serif".into());
            s.font_size = Some(Len::Px(16.0));
            s.line_height = Some(Len::Num(1.4));
            s.color = Some(spark_core::color::Color::BLACK);
        }
        NodeKind::Frame | NodeKind::Rect => {
            s.width = size(w.max(1.0));
            s.height = size(h.max(1.0));
            let fill = if kind == NodeKind::Frame { "#ffffff" } else { "#d9d9d9" };
            s.fills = vec![Fill::Solid { color: fill.parse().unwrap(), hidden: false }];
            if kind == NodeKind::Frame && parent_is_page(parent) {
                s.overflow = Some("clip".into());
            }
        }
        NodeKind::Svg | NodeKind::Vector => {}
    }
    node
}

fn parent_is_page(p: Parent) -> bool {
    matches!(p, Parent::Page(_))
}

/// Imports HTML (with images already loaded) under `place` as one undo step.
pub fn paste_html(
    open: &mut OpenDoc,
    html: &str,
    place: edit::Placement,
    loaded: &std::collections::HashMap<String, crate::images::Loaded>,
) -> CmdResult {
    let mut resolve = crate::images::resolver(loaded);
    let imp = spark_core::html::import(&mut open.editor.doc, html, place.parent, None, &mut resolve);
    if imp.roots.is_empty() {
        return Err(if imp.notes.is_empty() { "nothing to paste".into() } else { imp.notes.join("; ") });
    }
    let mut ops = imp.ops;
    edit::position_roots(&open.editor.doc, &mut ops, &imp.roots, &place, &[]);
    let ops = open.editor.transact(ops, None).map_err(|e| e.to_string())?;
    open.editor.selection = imp.roots.clone();
    Ok((json!({"ids": imp.roots, "notes": imp.notes}), ops))
}

/// Runs one UI command from window `client`. Returns the reply payload and the ops that were applied.
pub fn run(open: &mut OpenDoc, client: ClientId, cmd: &Value) -> CmdResult {
    let result = run_cmd(open, client, cmd);
    // A gesture and its undo group end together. Undo closes the group under a drag, so the drag is
    // over; a start or drop that failed halfway leaves a group with no gesture, so it is taken back.
    if !open.editor.in_group() {
        open.gesture = None;
    } else if open.gesture.is_none() {
        let undo = open.editor.cancel();
        open.commit_ops(&undo);
    }
    result
}

fn run_cmd(open: &mut OpenDoc, client: ClientId, cmd: &Value) -> CmdResult {
    let kind = cmd["type"].as_str().unwrap_or_default();
    let merge = cmd["merge"].as_str().map(str::to_string);
    let tx = |open: &mut OpenDoc, ops: Vec<Op>| open.editor.transact(ops, merge.as_deref()).map_err(|e| e.to_string());
    match kind {
        "undo" => Ok((json!(null), open.editor.undo().unwrap_or_default())),
        "redo" => Ok((json!(null), open.editor.redo().unwrap_or_default())),
        "begin" => {
            open.end_gesture();
            open.editor.begin();
            open.gesture = Some(Gesture { by: client, kind: GestureKind::Styling });
            Ok((json!(null), vec![]))
        }
        "commit" | "resizeEnd" => {
            open.end_gesture();
            Ok((json!(null), vec![]))
        }
        "cancel" => {
            open.gesture = None;
            Ok((json!(null), open.editor.cancel()))
        }
        "create" => {
            let c: Create = parse(cmd)?;
            let parent: Parent = c.parent.parse().map_err(|e: spark_core::id::IdError| e.to_string())?;
            let mut node = new_node(open, c.kind, parent, c.x, c.y, c.w, c.h);
            if let Some(t) = c.text {
                node.text = Some(t);
            }
            let id = node.id;
            let key = open.editor.doc.key_at(parent, c.index, None);
            let ops = tx(open, vec![Op::Insert { node, parent, key }])?;
            open.editor.selection = vec![id];
            Ok((json!({"id": id}), ops))
        }
        "setStyle" => {
            #[derive(Deserialize)]
            struct C {
                ids: Vec<String>,
                styles: serde_json::Map<String, Value>,
            }
            let c: C = parse(cmd)?;
            // A style handle's frames arriving after its gesture ended (undo mid-drag) are dropped,
            // or each would become its own undo step.
            if cmd["gesture"] == true && !matches!(open.gesture, Some(Gesture { kind: GestureKind::Styling, .. })) {
                return Err("no gesture in progress".into());
            }
            let mut ignored = Vec::new();
            let mut ops = Vec::new();
            for id in ids(&c.ids)? {
                ops.extend(style_ops(open, id, &c.styles, &mut ignored)?);
            }
            let ops = tx(open, ops)?;
            Ok((json!({"ignored": ignored}), ops))
        }
        "set" => {
            #[derive(Deserialize)]
            struct C {
                ids: Vec<String>,
                prop: String,
                value: Value,
            }
            let c: C = parse(cmd)?;
            let mut ops = Vec::new();
            for id in ids(&c.ids)? {
                ops.push(Op::Set { id, prop: c.prop.clone(), value: c.value.clone() });
                // Text layers named after their content keep following it until renamed.
                if let (Some(n), "text", Some(text)) = (open.editor.doc.node(id), c.prop.as_str(), c.value.as_str()) {
                    let auto = |t: &str| t.chars().take(40).collect::<String>().trim().to_string();
                    if n.name == "Text" || n.name == auto(n.text.as_deref().unwrap_or("")) {
                        let name = auto(text);
                        if !name.is_empty() {
                            ops.push(Op::Set { id, prop: "name".into(), value: json!(name) });
                        }
                    }
                }
            }
            Ok((json!(null), tx(open, ops)?))
        }
        "replaceColor" => {
            #[derive(Deserialize)]
            struct C {
                ids: Vec<String>,
                from: Color,
                to: Color,
            }
            let c: C = parse(cmd)?;
            let doc = &open.editor.doc;
            let mut ops = Vec::new();
            for id in ids(&c.ids)? {
                for n in std::iter::once(id).chain(doc.descendants(Parent::Node(id))) {
                    let Some(node) = doc.node(n) else { continue };
                    let mut style = node.style.clone();
                    replace_color(&mut style, &c.from, &c.to);
                    ops.extend(style_diff(n, &node.style, &style));
                }
            }
            Ok((json!(null), tx(open, ops)?))
        }
        "delete" => {
            #[derive(Deserialize)]
            struct C {
                ids: Vec<String>,
            }
            let c: C = parse(cmd)?;
            let doc = &open.editor.doc;
            let list = ids(&c.ids)?;
            // Skip nodes whose ancestor is also being deleted.
            let ops: Vec<Op> = list
                .iter()
                .filter(|id| doc.node(**id).is_some() && !doc.ancestors(**id).iter().any(|a| list.contains(a)))
                .map(|id| Op::Delete { id: *id })
                .collect();
            open.editor.selection.retain(|s| !list.contains(s));
            Ok((json!(null), tx(open, ops)?))
        }
        "dragStart" => {
            let mut list = ids(&parse::<Vec<String>>(&cmd["ids"])?)?;
            // A start can outrun the end of the gesture before it.
            open.end_gesture();
            open.editor.begin();
            let mut ops = Vec::new();
            if cmd["duplicate"].as_bool().unwrap_or(false) {
                let (new_ids, dup, _) = edit::duplicate(open, &list, false)?;
                ops = tx(open, dup)?;
                list = new_ids;
                open.editor.selection = list.clone();
            }
            let session = edit::session(&open.editor.doc, &list)?;
            open.gesture = Some(Gesture { by: client, kind: GestureKind::Moving(session) });
            Ok((json!({"ids": list}), ops))
        }
        "drag" => {
            let point = cmd["x"].as_f64().zip(cmd["y"].as_f64());
            edit::drag(
                open,
                cmd["dx"].as_f64().unwrap_or(0.0),
                cmd["dy"].as_f64().unwrap_or(0.0),
                point,
                cmd["snap"].as_bool().unwrap_or(true),
                cmd["pixel"].as_bool().unwrap_or(false),
                cmd["zoom"].as_f64().unwrap_or(1.0),
            )
        }
        "dragEnd" => {
            let point = cmd["x"].as_f64().zip(cmd["y"].as_f64());
            let ops = edit::end_drag(open, point, cmd["reparent"].as_bool().unwrap_or(false))?;
            Ok((json!(null), ops))
        }
        "resizeStart" => {
            let list = ids(&parse::<Vec<String>>(&cmd["ids"])?)?;
            open.end_gesture();
            let session = edit::session(&open.editor.doc, &list)?;
            open.editor.begin();
            open.gesture = Some(Gesture { by: client, kind: GestureKind::Resizing(session) });
            Ok((json!(null), vec![]))
        }
        "resize" => {
            let rect: spark_core::Rect = parse(&cmd["rect"])?;
            edit::resize(
                open,
                rect,
                cmd["snap"].as_bool().unwrap_or(true),
                cmd["pixel"].as_bool().unwrap_or(false),
                cmd["zoom"].as_f64().unwrap_or(1.0),
            )
        }
        "nudge" => {
            let list = ids(&parse::<Vec<String>>(&cmd["ids"])?)?;
            let ops = edit::nudge(open, &list, cmd["dx"].as_f64().unwrap_or(0.0), cmd["dy"].as_f64().unwrap_or(0.0));
            Ok((json!(null), tx(open, ops)?))
        }
        "duplicate" => {
            let list = ids(&parse::<Vec<String>>(&cmd["ids"])?)?;
            let (new_ids, ops, _) = edit::duplicate(open, &list, true)?;
            let ops = tx(open, ops)?;
            open.editor.selection = new_ids.clone();
            Ok((json!({"ids": new_ids}), ops))
        }
        "align" | "distribute" => {
            let list = ids(&parse::<Vec<String>>(&cmd["ids"])?)?;
            let arg = cmd["dir"].as_str().unwrap_or_default();
            let doc = &open.editor.doc;
            let moves =
                if kind == "align" { edit::align(doc, &list, arg) } else { edit::distribute(doc, &list, arg == "x") };
            let ops = tx(open, edit::shift_ops(doc, &moves))?;
            edit::carry_rects(&mut open.editor.doc, &moves);
            Ok((json!(null), ops))
        }
        "transform" | "swapColors" => {
            let list = ids(&parse::<Vec<String>>(&cmd["ids"])?)?;
            let doc = &open.editor.doc;
            let ops = match cmd["dir"].as_str() {
                Some(op) if kind == "transform" => edit::transform(doc, &list, op),
                _ => edit::swap_colors(doc, &list),
            };
            Ok((json!(null), tx(open, ops)?))
        }
        "ungroup" => {
            let list = ids(&parse::<Vec<String>>(&cmd["ids"])?)?;
            let (freed, ops) = edit::ungroup(&open.editor.doc, &list);
            let ops = tx(open, ops)?;
            if !freed.is_empty() {
                open.editor.selection = freed.clone();
            }
            Ok((json!({"ids": freed}), ops))
        }
        "wrap" => {
            let list = ids(&parse::<Vec<String>>(&cmd["ids"])?)?;
            let (id, ops) = edit::wrap(open, &list)?;
            let ops = tx(open, ops)?;
            open.editor.selection = vec![id];
            Ok((json!({"id": id}), ops))
        }
        "flex" => {
            let list = ids(&parse::<Vec<String>>(&cmd["ids"])?)?;
            let doc = &open.editor.doc;
            let single_frame = list.len() == 1 && doc.node(list[0]).is_some_and(|n| n.kind == NodeKind::Frame);
            if single_frame {
                let ops = edit::add_flex(doc, list[0]);
                return Ok((json!({"id": list[0]}), tx(open, ops)?));
            }
            // Wrap the selection in a new flex frame.
            open.editor.begin();
            let (id, ops) = edit::wrap(open, &list)?;
            let mut done = tx(open, ops)?;
            let flex = edit::add_flex(&open.editor.doc, id);
            done.extend(tx(open, flex)?);
            open.editor.commit();
            open.editor.selection = vec![id];
            Ok((json!({"id": id}), done))
        }
        "freeform" => {
            let list = ids(&parse::<Vec<String>>(&cmd["ids"])?)?;
            let ops: Vec<Op> = list.iter().flat_map(|id| edit::remove_flex(&open.editor.doc, *id)).collect();
            Ok((json!(null), tx(open, ops)?))
        }
        "zorder" => {
            let list = ids(&parse::<Vec<String>>(&cmd["ids"])?)?;
            let ops = edit::zorder(&open.editor.doc, &list, cmd["dir"].as_str().unwrap_or_default());
            Ok((json!(null), tx(open, ops)?))
        }
        "move" => {
            let list = ids(&parse::<Vec<String>>(&cmd["ids"])?)?;
            let parent: Parent = parse(&cmd["parent"])?;
            let index = cmd["index"].as_u64().map(|i| i as usize);
            let doc = &open.editor.doc;
            let world: Vec<(NodeId, spark_core::Rect)> =
                list.iter().filter_map(|id| Some((*id, *doc.measured.get(id)?))).collect();
            let mut done = Vec::new();
            open.editor.begin();
            for (i, id) in list.iter().enumerate() {
                let doc = &open.editor.doc;
                let same_parent = doc.parent(*id) == Some(parent);
                let ops = if same_parent {
                    vec![Op::Move { id: *id, parent, key: doc.key_at(parent, index.map(|n| n + i), Some(*id)) }]
                } else {
                    edit::reparent_ops(doc, &[*id], parent, index.map(|n| n + i), &world)
                };
                match tx(open, ops) {
                    Ok(o) => done.extend(o),
                    Err(e) => {
                        let undo = open.editor.cancel();
                        open.commit_ops(&undo);
                        return Err(e);
                    }
                }
            }
            open.editor.commit();
            Ok((json!(null), done))
        }
        "code" => {
            let list = ids(&parse::<Vec<String>>(&cmd["ids"])?)?;
            if cmd["format"] == "svg" {
                let svgs = list
                    .iter()
                    .map(|id| crate::export::node_svg(&open.editor.doc, *id))
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok((json!(svgs.join("\n")), vec![]));
            }
            let format = match cmd["format"].as_str().unwrap_or("html") {
                "jsx" => Format::Jsx,
                "tailwind" => Format::Tailwind,
                _ => Format::Html,
            };
            let opts = CodeOpts {
                format,
                include_ids: false,
                root_positioning: false,
                asset_url: &|h| format!("/assets/{h}"),
            };
            Ok((json!(codegen::code(&open.editor.doc, &list, &opts)), vec![]))
        }
        "pasteSpark" if cmd["replace"].is_array() => {
            let roots = cmd["roots"].as_array().cloned().ok_or("roots required")?;
            let targets = ids(&parse::<Vec<String>>(&cmd["replace"])?)?;
            let (new_ids, ops) = edit::replace_ops(&mut open.editor.doc, &roots, &targets)?;
            let ops = tx(open, ops)?;
            open.editor.selection = new_ids.clone();
            Ok((json!({"ids": new_ids}), ops))
        }
        "pasteSpark" => {
            let parent: Parent = parse(&cmd["parent"])?;
            let roots = cmd["roots"].as_array().cloned().ok_or("roots required")?;
            let at = cmd["x"].as_f64().zip(cmd["y"].as_f64());
            let (new_ids, mut ops, rects) = edit::spark_paste_ops(&mut open.editor.doc, &roots, parent)?;
            let same_parent = cmd["from"].as_str().is_some_and(|f| f == parent.to_string());
            let place = edit::Placement { parent, at: if same_parent && at.is_none() { None } else { at } };
            if !same_parent || at.is_some() || matches!(parent, Parent::Page(_)) {
                edit::position_roots(&open.editor.doc, &mut ops, &new_ids, &place, &rects);
            }
            let ops = tx(open, ops)?;
            open.editor.selection = new_ids.clone();
            Ok((json!({"ids": new_ids}), ops))
        }
        "pasteText" => {
            let parent: Parent = parse(&cmd["parent"])?;
            let (x, y) = (cmd["x"].as_f64().unwrap_or(0.0), cmd["y"].as_f64().unwrap_or(0.0));
            let mut node = new_node(open, NodeKind::Text, parent, x, y, 0.0, 0.0);
            node.text = Some(cmd["text"].as_str().unwrap_or_default().to_string());
            node.name = node.text.as_deref().unwrap_or("Text").chars().take(40).collect();
            let id = node.id;
            let key = open.editor.doc.key_at(parent, None, None);
            let ops = tx(open, vec![Op::Insert { node, parent, key }])?;
            open.editor.selection = vec![id];
            Ok((json!({"ids": [id]}), ops))
        }
        "pasteStyle" => {
            // Appearance fields go to every target; type fields only to text layers, and only when
            // the copied layer was text. Fields the copied style lacks are cleared.
            const LOOK: [&str; 8] =
                ["fills", "border", "radius", "shadows", "opacity", "blend", "blur", "backdropBlur"];
            const TYPE: [&str; 8] = [
                "fontFamily",
                "fontSize",
                "fontWeight",
                "fontStyle",
                "lineHeight",
                "letterSpacing",
                "textAlign",
                "color",
            ];
            let list = ids(&parse::<Vec<String>>(&cmd["ids"])?)?;
            let style = cmd["style"].as_object().ok_or("style required")?;
            let with_type = cmd["text"] == true;
            let mut ops = Vec::new();
            for id in list {
                let Some(n) = open.editor.doc.node(id) else { continue };
                let typed = if with_type && n.kind == NodeKind::Text { &TYPE[..] } else { &[] };
                for f in LOOK.iter().chain(typed) {
                    let value = style.get(*f).cloned().unwrap_or(Value::Null);
                    if n.style.get_field(f) != value {
                        ops.push(Op::Set { id, prop: format!("style.{f}"), value });
                    }
                }
            }
            Ok((json!(null), tx(open, ops)?))
        }
        "adjustText" => {
            let list = ids(&parse::<Vec<String>>(&cmd["ids"])?)?;
            let prop = cmd["prop"].as_str().unwrap_or_default();
            let dir = cmd["dir"].as_f64().unwrap_or(0.0).signum();
            let ops: Vec<Op> = list
                .into_iter()
                .filter_map(|id| {
                    let n = open.editor.doc.node(id).filter(|n| n.kind == NodeKind::Text)?;
                    Some(Op::Set { id, prop: format!("style.{prop}"), value: edit::adjust_text(&n.style, prop, dir)? })
                })
                .collect();
            Ok((json!(null), tx(open, ops)?))
        }
        "renameFile" => {
            let name = cmd["name"].as_str().unwrap_or_default().to_string();
            Ok((json!(null), tx(open, vec![Op::SetFileName { name }])?))
        }
        "addPage" => {
            let divider = cmd["divider"].as_bool().unwrap_or(false);
            let doc = &mut open.editor.doc;
            let n = doc.pages.iter().filter(|p| !p.divider).count() + 1;
            let name = cmd["name"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| if divider { String::new() } else { format!("Page {n}") });
            let id = doc.alloc_page();
            let after = open.editor.page.and_then(|p| open.editor.doc.pages.iter().position(|x| x.id == p));
            let index = cmd["index"].as_u64().map(|i| i as usize).or(after.map(|i| i + 1)).unwrap_or(usize::MAX);
            let page = Page { id, name, icon: None, canvas: None, divider };
            let ops = tx(open, vec![Op::InsertPage { page, index }])?;
            if !divider {
                open.editor.page = Some(id);
            }
            Ok((json!({"id": id}), ops))
        }
        "duplicatePage" => {
            let id: PageId = parse(&cmd["id"])?;
            let doc = &mut open.editor.doc;
            let src = doc.page(id).filter(|p| !p.divider).cloned().ok_or("page not found")?;
            let index = doc.pages.iter().position(|p| p.id == id).unwrap() + 1;
            let new_id = doc.alloc_page();
            let page = Page { id: new_id, name: format!("{} copy", src.name), ..src };
            let mut ops = vec![Op::InsertPage { page, index }];
            for (key, board) in doc.child_keys(Parent::Page(id)).to_vec() {
                ops.extend(edit::clone_ops(doc, board, Parent::Page(new_id), key).1);
            }
            let ops = tx(open, ops)?;
            open.editor.page = Some(new_id);
            Ok((json!({"id": new_id}), ops))
        }
        "setPage" => {
            let id: PageId = parse(&cmd["id"])?;
            let prop = cmd["prop"].as_str().unwrap_or_default().to_string();
            Ok((json!(null), tx(open, vec![Op::SetPage { id, prop, value: cmd["value"].clone() }])?))
        }
        "deletePage" => {
            let id: PageId = parse(&cmd["id"])?;
            let ops = tx(open, vec![Op::DeletePage { id }])?;
            if open.editor.page == Some(id) {
                open.editor.page = open.editor.doc.first_page();
            }
            Ok((json!(null), ops))
        }
        "movePage" => {
            let id: PageId = parse(&cmd["id"])?;
            let index = cmd["index"].as_u64().ok_or("index required")? as usize;
            Ok((json!(null), tx(open, vec![Op::MovePage { id, index }])?))
        }
        "setToken" => {
            #[derive(Deserialize)]
            struct C {
                name: String,
                #[serde(default, rename = "newName")]
                new_name: Option<String>,
                value: Option<String>,
                #[serde(default)]
                description: Option<String>,
            }
            let c: C = parse(cmd)?;
            let doc = &open.editor.doc;
            let old = doc.tokens.get(&c.name).cloned();
            let value = c.value.or(old.as_ref().map(|t| t.value.clone())).ok_or("token value required")?;
            let description = c.description.or(old.and_then(|t| t.description)).filter(|d| !d.is_empty());
            let name = c.new_name.unwrap_or(c.name.clone());
            let mut ops = Vec::new();
            if name != c.name {
                ops.push(Op::SetToken { name: c.name.clone(), token: None });
            }
            ops.push(Op::SetToken { name: name.clone(), token: Some(Token { name, value, description }) });
            Ok((json!(null), tx(open, ops)?))
        }
        "deleteToken" => {
            let name = cmd["name"].as_str().unwrap_or_default().to_string();
            Ok((json!(null), tx(open, vec![Op::SetToken { name, token: None }])?))
        }
        "setGuide" => {
            let text = cmd["text"].as_str().ok_or("text required")?.to_string();
            Ok((json!(null), tx(open, vec![Op::SetGuide { text }])?))
        }
        "designMd" => Ok((json!(spark_core::export::design_md(&open.editor.doc)), vec![])),
        "tokensCss" => {
            let theme = cmd["theme"].as_bool().unwrap_or(true);
            Ok((json!(spark_core::tokens::to_css(&open.editor.doc.tokens, theme)), vec![]))
        }
        other => Err(format!("unknown command `{other}`")),
    }
}
