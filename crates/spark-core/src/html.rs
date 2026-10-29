//! HTML import: turns HTML with inline `style` attributes into nodes. This is how agents write
//! designs (`write_html`) and how pasted HTML becomes layers.
//!
//! Rules: `<svg>` becomes an svg node with vector children; `<img>` becomes a rectangle with an
//! image fill; an element holding only text and inline formatting becomes one text node; every
//! other element becomes a frame. `layer-name` sets the layer name and
//! `<spark-clone node-id="…">` copies an existing node. Only inline styles are read.

use scraper::{ElementRef, Html, Node as HNode};
use serde_json::Value;

use crate::{
    css::{apply_decl, parse_declarations},
    doc::{Document, MAX_DEPTH, Op, Parent},
    fractional::keys_between,
    id::NodeId,
    node::{Node, NodeKind},
    style::{Fill, Fit, Len, Size, Style},
};

/// What the caller made of an image URL.
pub enum ImageSrc {
    /// Stored (or already present) asset. `bytes` is set when it still needs adding.
    Asset { hash: String, mime: String, bytes: Option<Vec<u8>>, width: Option<f64>, height: Option<f64> },
    /// An image request or generation job to attach to the node (spark-gen URLs).
    Job(Value),
    /// Could not load; the message is reported back.
    Failed(String),
}

/// An image as an asset hash plus its natural size, or a generation job.
type Resolved = Result<(String, Option<(f64, f64)>), Value>;

#[derive(Debug, Default)]
pub struct Import {
    pub ops: Vec<Op>,
    pub roots: Vec<NodeId>,
    /// Ignored styles, failed images and other things the caller should report.
    pub notes: Vec<String>,
}

const INLINE: &[&str] = &[
    "a", "abbr", "b", "bdi", "bdo", "cite", "code", "data", "dfn", "em", "i", "kbd", "mark", "q", "s", "samp", "small",
    "span", "strong", "sub", "sup", "time", "u", "var", "br", "wbr", "del", "ins",
];
const SKIP: &[&str] = &["script", "style", "template", "noscript", "head", "meta", "link", "title"];

struct Ctx<'d, 'r> {
    doc: &'d mut Document,
    resolve: &'r mut dyn FnMut(&str) -> ImageSrc,
    ops: Vec<Op>,
    notes: Vec<String>,
    added_assets: std::collections::HashSet<String>,
}

/// Properties that only change how text looks, so an inline element carrying them can merge into
/// its parent's text node.
fn text_only_style(css: &str) -> bool {
    parse_declarations(css).iter().all(|(k, _)| {
        k == "color"
            || k.starts_with("font")
            || k.starts_with("text-")
            || k == "letter-spacing"
            || k == "line-height"
            || k == "white-space"
    })
}

/// Whether an element holds only text and inline formatting, so it becomes one text node.
fn is_inline_only(el: ElementRef) -> bool {
    let own = el.value().attr("style").map(parse_declarations).unwrap_or_default();
    let layout = own.iter().any(|(k, v)| k == "display" && (v.contains("flex") || v.contains("grid")));
    el.children().all(|c| match c.value() {
        HNode::Text(_) | HNode::Comment(_) => true,
        HNode::Element(e) => {
            !layout
                && INLINE.contains(&e.name())
                && e.attr("style").is_none_or(text_only_style)
                && ElementRef::wrap(c).is_some_and(is_inline_only)
        }
        _ => true,
    })
}

const BREAK: char = '\u{2028}';

/// Text content with `<br>` as newlines and other whitespace collapsed like `white-space: normal`.
fn text_of(el: ElementRef, pre: bool) -> String {
    fn walk(el: ElementRef, out: &mut String) {
        for c in el.children() {
            match c.value() {
                HNode::Text(t) => out.push_str(t),
                HNode::Element(e) if e.name() == "br" => out.push(BREAK),
                HNode::Element(_) => {
                    if let Some(e) = ElementRef::wrap(c) {
                        walk(e, out)
                    }
                }
                _ => {}
            }
        }
    }
    let mut raw = String::new();
    walk(el, &mut raw);
    if pre {
        return raw.replace(BREAK, "\n");
    }
    let mut out = String::with_capacity(raw.len());
    let mut space = false;
    for ch in raw.chars() {
        if ch == BREAK {
            out.truncate(out.trim_end_matches(' ').len());
            out.push('\n');
            space = false;
        } else if ch.is_whitespace() && ch != '\u{a0}' {
            space = true;
        } else {
            if space && !out.is_empty() && !out.ends_with('\n') {
                out.push(' ');
            }
            space = false;
            out.push(ch);
        }
    }
    out
}

fn heading_defaults(tag: &str, style: &mut Style) {
    let size = match tag {
        "h1" => 32.0,
        "h2" => 24.0,
        "h3" => 18.72,
        "h4" => 16.0,
        "h5" => 13.28,
        "h6" => 10.72,
        _ => return,
    };
    style.font_size.get_or_insert(Len::Px(size));
    style.font_weight.get_or_insert(700);
}

fn display_name(tag: &str) -> String {
    match tag {
        "div" | "span" | "p" => "Frame".into(),
        t => {
            let mut c = t.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_else(|| "Frame".into())
        }
    }
}

impl Ctx<'_, '_> {
    /// Rewrites `url(...)` references to stored assets; returns false if any failed.
    fn rewrite_urls(&mut self, value: &str) -> String {
        let mut out = String::new();
        let mut rest = value;
        while let Some(i) = rest.find("url(") {
            out.push_str(&rest[..i]);
            let after = &rest[i + 4..];
            let Some(end) = after.find(')') else { break };
            let url = after[..end].trim().trim_matches(['"', '\'']);
            match self.image(url) {
                Some(Ok(hash)) => out.push_str(&format!("url(/assets/{hash})")),
                _ => out.push_str(&format!("url({url})")),
            }
            rest = &after[end + 1..];
        }
        out.push_str(rest);
        out
    }

    /// Resolves an image URL to an asset hash (Ok) or a generation job (Err).
    fn image(&mut self, url: &str) -> Option<Result<String, Value>> {
        self.image_full(url).map(|r| r.map(|(hash, _)| hash))
    }

    fn style_of(&mut self, el: ElementRef) -> Style {
        let mut style = Style::default();
        if let Some(css) = el.value().attr("style") {
            for (prop, value) in parse_declarations(css) {
                let value = if value.contains("url(") { self.rewrite_urls(&value) } else { value };
                if let Err(e) = apply_decl(&mut style, &prop, &value) {
                    self.notes.push(e);
                }
            }
        }
        style
    }

    fn insert(&mut self, node: Node, parent: Parent, key: String) -> NodeId {
        let id = node.id;
        self.ops.push(Op::Insert { node, parent, key });
        id
    }

    /// Converts one element. Returns the new node id, or None if it was skipped.
    fn element(&mut self, el: ElementRef, parent: Parent, key: String) -> Option<NodeId> {
        let tag = el.value().name().to_ascii_lowercase();
        if SKIP.contains(&tag.as_str()) {
            return None;
        }
        let name_attr = el.value().attr("layer-name").map(str::to_string);
        if tag == "spark-clone" {
            return self.clone_node(el, parent, key);
        }
        let mut style = self.style_of(el);
        heading_defaults(&tag, &mut style);
        if matches!(tag.as_str(), "b" | "strong") {
            style.font_weight.get_or_insert(700);
        }
        if matches!(tag.as_str(), "i" | "em") {
            style.font_style.get_or_insert("italic".into());
        }
        let id = self.doc.alloc_node();
        if tag == "svg" {
            let mut node = Node::new(id, NodeKind::Svg);
            node.name = name_attr.unwrap_or_else(|| "SVG".into());
            node.attrs = el
                .value()
                .attrs()
                .filter(|(k, _)| !matches!(*k, "style" | "layer-name"))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            for (attr, axis) in [("width", true), ("height", false)] {
                let has = if axis { style.width.is_some() } else { style.height.is_some() };
                if let (false, Some(v)) = (has, el.value().attr(attr).and_then(|v| Len::parse(v, false))) {
                    if axis { style.width = Some(Size::Fixed(v)) } else { style.height = Some(Size::Fixed(v)) }
                }
            }
            node.style = style;
            let nid = self.insert(node, parent, key);
            self.vectors(el, nid);
            return Some(nid);
        }
        if tag == "img" {
            let mut node = Node::new(id, NodeKind::Rect);
            node.name = name_attr
                .or_else(|| el.value().attr("alt").filter(|a| !a.is_empty()).map(str::to_string))
                .unwrap_or_else(|| "Image".into());
            let fit = match style.raw.remove("object-fit").as_deref() {
                Some("contain") => Fit::Contain,
                Some("fill") => Fit::Fill,
                _ => Fit::Cover,
            };
            let src = el.value().attr("src").unwrap_or_default().to_string();
            let mut natural = None;
            match self.image_full(&src) {
                Some(Ok((hash, size))) => {
                    natural = size;
                    style.fills.insert(0, Fill::Image { asset: hash, fit, hidden: false });
                }
                Some(Err(job)) => {
                    natural = job["width"].as_f64().zip(job["height"].as_f64());
                    node.image = Some(job);
                }
                None => {}
            }
            for (attr, axis) in [("width", true), ("height", false)] {
                let has = if axis { style.width.is_some() } else { style.height.is_some() };
                if !has && let Some(v) = el.value().attr(attr).and_then(|v| Len::parse(v, false)) {
                    if axis { style.width = Some(Size::Fixed(v)) } else { style.height = Some(Size::Fixed(v)) }
                }
            }
            // Like a browser, a missing side follows the image's aspect ratio.
            if let Some((nw, nh)) = natural.filter(|(w, h)| *w > 0.0 && *h > 0.0) {
                let px = |s: &Option<Size>| match s {
                    Some(Size::Fixed(Len::Px(v))) => Some(*v),
                    _ => None,
                };
                match (px(&style.width), px(&style.height), style.width.is_some(), style.height.is_some()) {
                    (Some(w), None, _, false) => style.height = Some(Size::Fixed(Len::Px((w * nh / nw).round()))),
                    (None, Some(h), false, _) => style.width = Some(Size::Fixed(Len::Px((h * nw / nh).round()))),
                    (None, None, false, false) => {
                        style.width = Some(Size::Fixed(Len::Px(nw)));
                        style.height = Some(Size::Fixed(Len::Px(nh)));
                    }
                    _ => {}
                }
            }
            node.style = style;
            return Some(self.insert(node, parent, key));
        }
        let pre = style.raw.get("white-space").is_some_and(|w| w.starts_with("pre"));
        let has_elements = el.children().any(|c| matches!(c.value(), HNode::Element(_)));
        let text = text_of(el, pre);
        if is_inline_only(el) && (!text.trim().is_empty() || !has_elements) && !text.is_empty() {
            let mut node = Node::new(id, NodeKind::Text);
            node.name = name_attr.unwrap_or_else(|| text.chars().take(40).collect::<String>().trim().to_string());
            node.text = Some(text);
            // Text nodes already render with pre-wrap.
            if style.raw.get("white-space").is_some_and(|w| w == "pre-wrap") {
                style.raw.remove("white-space");
            }
            node.style = style;
            return Some(self.insert(node, parent, key));
        }
        let mut node = Node::new(id, NodeKind::Frame);
        node.name = name_attr.unwrap_or_else(|| display_name(&tag));
        node.style = style;
        let nid = self.insert(node, parent, key);
        self.children(el, Parent::Node(nid));
        Some(nid)
    }

    fn image_full(&mut self, url: &str) -> Option<Resolved> {
        if url.is_empty() {
            self.notes.push("an <img> has no src".into());
            return None;
        }
        if let Some(hash) = url.strip_prefix("/assets/") {
            return Some(Ok((hash.to_string(), None)));
        }
        match (self.resolve)(url) {
            ImageSrc::Asset { hash, mime, bytes, width, height } => {
                if let Some(bytes) = bytes
                    && self.added_assets.insert(hash.clone())
                {
                    self.ops.push(Op::AddAsset { hash: hash.clone(), mime, bytes: std::sync::Arc::new(bytes) });
                }
                Some(Ok((hash, width.zip(height))))
            }
            ImageSrc::Job(v) => Some(Err(v)),
            ImageSrc::Failed(msg) => {
                self.notes.push(msg);
                None
            }
        }
    }

    /// Children of an element: elements convert recursively, loose text becomes text nodes.
    fn children(&mut self, el: ElementRef, parent: Parent) {
        let items: Vec<_> = el
            .children()
            .filter(|c| match c.value() {
                HNode::Element(_) => true,
                HNode::Text(t) => !t.trim().is_empty(),
                _ => false,
            })
            .collect();
        let keys = keys_between(None, None, items.len());
        for (c, key) in items.into_iter().zip(keys) {
            match c.value() {
                HNode::Element(_) => {
                    if let Some(e) = ElementRef::wrap(c) {
                        self.element(e, parent, key);
                    }
                }
                HNode::Text(t) => {
                    let text = t.split_whitespace().collect::<Vec<_>>().join(" ");
                    let mut node = Node::new(self.doc.alloc_node(), NodeKind::Text);
                    node.name = text.chars().take(40).collect();
                    node.text = Some(text);
                    self.insert(node, parent, key);
                }
                _ => {}
            }
        }
    }

    fn vectors(&mut self, el: ElementRef, parent: NodeId) {
        let kids: Vec<ElementRef> = el.children().filter_map(ElementRef::wrap).collect();
        let keys = keys_between(None, None, kids.len());
        for (c, key) in kids.into_iter().zip(keys) {
            let mut node = Node::new(self.doc.alloc_node(), NodeKind::Vector);
            let tag = c.value().name().to_string();
            node.name = c.value().attr("layer-name").map(str::to_string).unwrap_or_else(|| tag.clone());
            node.tag = Some(tag);
            node.attrs = c
                .value()
                .attrs()
                .filter(|(k, _)| *k != "layer-name")
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            // Text inside svg elements (<text>, <title>) is kept as an attribute-free child run.
            let inner: String = c.children().filter_map(|t| t.value().as_text().map(|t| t.to_string())).collect();
            if !inner.trim().is_empty() {
                node.attrs.insert("data-text".into(), inner.trim().to_string());
            }
            let id = self.insert(node, Parent::Node(parent), key);
            self.vectors(c, id);
        }
    }

    fn clone_node(&mut self, el: ElementRef, parent: Parent, key: String) -> Option<NodeId> {
        let src: Option<NodeId> = el.value().attr("node-id").and_then(|s| s.parse().ok());
        let Some(src) = src.filter(|s| self.doc.node(*s).is_some()) else {
            self.notes.push(format!("spark-clone: node {} not found", el.value().attr("node-id").unwrap_or("?")));
            return None;
        };
        let mut stack = vec![(src, parent, key, true)];
        let mut root = None;
        while let Some((s, parent, key, is_root)) = stack.pop() {
            let mut node = self.doc.node(s).cloned()?;
            node.id = self.doc.alloc_node();
            if is_root {
                root = Some(node.id);
                if let Some(name) = el.value().attr("layer-name") {
                    node.name = name.into();
                }
                // Inline styles on the clone element override the copy's own.
                if let Some(css) = el.value().attr("style") {
                    for (p, v) in parse_declarations(css) {
                        let _ = apply_decl(&mut node.style, &p, &v);
                    }
                }
            }
            for (k, child) in self.doc.child_keys(Parent::Node(s)).to_vec() {
                stack.push((child, Parent::Node(node.id), k, false));
            }
            self.ops.push(Op::Insert { node, parent, key });
        }
        root
    }
}

/// Parses `html` and returns ops that insert it under `parent` at `index` (None = end).
/// Ops are ordered parents first; nothing is applied to `doc` except id allocation.
pub fn import(
    doc: &mut Document,
    html: &str,
    parent: Parent,
    index: Option<usize>,
    resolve: &mut dyn FnMut(&str) -> ImageSrc,
) -> Import {
    let fragment = Html::parse_fragment(html);
    let root = fragment.root_element();
    // Conversion recurses per element; anything this deep would be refused when applied anyway.
    if root.descendants().any(|n| n.ancestors().nth(MAX_DEPTH + 2).is_some()) {
        let note = format!("the HTML nests more than {MAX_DEPTH} levels deep");
        return Import { ops: Vec::new(), roots: Vec::new(), notes: vec![note] };
    }
    let top: Vec<ElementRef> =
        root.children().filter_map(ElementRef::wrap).filter(|e| !SKIP.contains(&e.value().name())).collect();
    let loose_text: String = root.children().filter_map(|c| c.value().as_text().map(|t| t.to_string())).collect();
    let mut keys = {
        let list = doc.child_keys(parent);
        let i = index.unwrap_or(list.len()).min(list.len());
        let before = i.checked_sub(1).map(|j| list[j].0.clone());
        let after = list.get(i).map(|e| e.0.clone());
        keys_between(before.as_deref(), after.as_deref(), top.len() + usize::from(!loose_text.trim().is_empty()))
    }
    .into_iter();
    let mut ctx = Ctx { doc, resolve, ops: Vec::new(), notes: Vec::new(), added_assets: Default::default() };
    let mut roots = Vec::new();
    if top.is_empty() && !loose_text.trim().is_empty() {
        let mut node = Node::new(ctx.doc.alloc_node(), NodeKind::Text);
        let text = loose_text.split_whitespace().collect::<Vec<_>>().join(" ");
        node.name = text.chars().take(40).collect();
        node.text = Some(text);
        roots.push(ctx.insert(node, parent, keys.next().unwrap()));
    }
    for el in top {
        if let Some(id) = ctx.element(el, parent, keys.next().unwrap()) {
            roots.push(id);
        }
    }
    Import { ops: ctx.ops, roots, notes: ctx.notes }
}
