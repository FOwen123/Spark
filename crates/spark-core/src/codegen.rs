//! Code export: HTML with inline styles, JSX with inline styles, and JSX with Tailwind classes.
//! The output is a direct translation of the compiled CSS, so it renders like the canvas.

use crate::{
    color::var_name,
    css::{ParentCtx, camel, compile, split_top},
    doc::{Document, Parent},
    id::NodeId,
    node::NodeKind,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Html,
    Jsx,
    Tailwind,
}

pub struct CodeOpts<'a> {
    pub format: Format,
    /// Add `id="<node id>"` to every element.
    pub include_ids: bool,
    /// Keep the roots' own position, left, top, right and bottom.
    pub root_positioning: bool,
    /// Maps an asset hash to the URL written into the code.
    pub asset_url: &'a dyn Fn(&str) -> String,
}

/// Compiled CSS for a node in context: parent layout, and `position: relative` on frames so their
/// absolutely positioned children stay inside them.
pub fn node_css(doc: &Document, id: NodeId) -> Vec<(String, String)> {
    let Some(node) = doc.node(id) else { return Vec::new() };
    let ctx = doc.parent_node(id).map(|p| ParentCtx::of(&p.style)).unwrap_or_default();
    let mut decls = compile(node, ctx);
    if node.kind == NodeKind::Frame && !decls.iter().any(|(k, _)| k == "position") {
        let abs_child = doc
            .children(Parent::Node(id))
            .any(|c| doc.node(c).is_some_and(|n| n.style.position.as_deref() == Some("absolute")));
        if abs_child {
            decls.push(("position".into(), "relative".into()));
        }
    }
    decls
}

fn esc_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn rewrite_assets(v: &str, url: &dyn Fn(&str) -> String) -> String {
    let mut out = String::new();
    let mut rest = v;
    while let Some(i) = rest.find("/assets/") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 8..];
        let end = after.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(after.len());
        out.push_str(&url(&after[..end]));
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

fn jsx_attr_name(k: &str) -> String {
    match k {
        "class" => "className".into(),
        "for" => "htmlFor".into(),
        k if k.starts_with("data-") || k.starts_with("aria-") => k.into(),
        k => {
            let k = k.replace(':', "-");
            camel(&k)
        }
    }
}

struct Gen<'a> {
    doc: &'a Document,
    opts: &'a CodeOpts<'a>,
    out: String,
}

impl Gen<'_> {
    fn line(&mut self, depth: usize, s: &str) {
        for _ in 0..depth {
            self.out.push_str("  ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    fn style_attr(&self, decls: &[(String, String)]) -> String {
        if decls.is_empty() {
            return String::new();
        }
        match self.opts.format {
            Format::Html => {
                let css = decls.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join("; ");
                format!(" style=\"{}\"", esc_html(&css))
            }
            Format::Jsx => {
                let body = decls
                    .iter()
                    .map(|(k, v)| {
                        let key = camel(k);
                        let key = if key.starts_with("--") { format!("\"{key}\"") } else { key };
                        format!("{key}: {}", serde_json::to_string(v).unwrap())
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(" style={{{{ {body} }}}}")
            }
            Format::Tailwind => {
                let classes: Vec<String> = decls.iter().flat_map(|(k, v)| tailwind(k, v)).collect();
                format!(" className=\"{}\"", classes.join(" "))
            }
        }
    }

    fn text(&self, s: &str) -> String {
        match self.opts.format {
            Format::Html => esc_html(s),
            _ if s.contains(['{', '}', '<', '>', '&', '\n', '"', '\'']) || s.starts_with(' ') || s.ends_with(' ') => {
                format!("{{{}}}", serde_json::to_string(s).unwrap())
            }
            _ => s.to_string(),
        }
    }

    fn node(&mut self, id: NodeId, depth: usize, root: bool) {
        let Some(n) = self.doc.node(id) else { return };
        if n.hidden {
            return;
        }
        let mut decls: Vec<(String, String)> =
            node_css(self.doc, id).into_iter().map(|(k, v)| (k, rewrite_assets(&v, self.opts.asset_url))).collect();
        if root && !self.opts.root_positioning {
            decls.retain(|(k, _)| !matches!(k.as_str(), "position" | "left" | "top" | "right" | "bottom"));
        }
        let mut id_attr = if self.opts.include_ids { format!(" id=\"{id}\"") } else { String::new() };
        // Theme names are validated slugs, so they need no escaping.
        if let Some(t) = &n.theme {
            id_attr.push_str(&format!(" data-theme=\"{t}\""));
        }
        let jsx = self.opts.format != Format::Html;
        match n.kind {
            NodeKind::Text => {
                let style = self.style_attr(&decls);
                let text = self.text(n.text.as_deref().unwrap_or(""));
                self.line(depth, &format!("<div{id_attr}{style}>{text}</div>"));
            }
            NodeKind::Svg | NodeKind::Vector => {
                let tag = if n.kind == NodeKind::Svg {
                    "svg".to_string()
                } else {
                    n.tag.clone().unwrap_or_else(|| "g".into())
                };
                let mut attrs = String::new();
                for (k, v) in &n.attrs {
                    if k == "data-text" {
                        continue;
                    }
                    let name = if jsx { jsx_attr_name(k) } else { k.clone() };
                    attrs.push_str(&format!(" {name}=\"{}\"", esc_html(v)));
                }
                let style = if n.kind == NodeKind::Svg { self.style_attr(&decls) } else { String::new() };
                let kids: Vec<NodeId> = self.doc.children(Parent::Node(id)).collect();
                let inner = n.attrs.get("data-text").map(|t| self.text(t));
                if kids.is_empty() && inner.is_none() {
                    self.line(depth, &format!("<{tag}{id_attr}{attrs}{style} />"));
                } else if kids.is_empty() {
                    self.line(depth, &format!("<{tag}{id_attr}{attrs}{style}>{}</{tag}>", inner.unwrap()));
                } else {
                    self.line(depth, &format!("<{tag}{id_attr}{attrs}{style}>"));
                    for k in kids {
                        self.node(k, depth + 1, false);
                    }
                    self.line(depth, &format!("</{tag}>"));
                }
            }
            NodeKind::Frame | NodeKind::Rect => {
                let style = self.style_attr(&decls);
                let kids: Vec<NodeId> = self
                    .doc
                    .children(Parent::Node(id))
                    .filter(|c| self.doc.node(*c).is_some_and(|n| !n.hidden))
                    .collect();
                if kids.is_empty() {
                    self.line(depth, &format!("<div{id_attr}{style}{}", if jsx { " />" } else { "></div>" }));
                } else {
                    self.line(depth, &format!("<div{id_attr}{style}>"));
                    for k in kids {
                        self.node(k, depth + 1, false);
                    }
                    self.line(depth, "</div>");
                }
            }
        }
    }
}

/// Code for the given nodes (each with its subtree), one root after another.
pub fn code(doc: &Document, ids: &[NodeId], opts: &CodeOpts) -> String {
    let mut g = Gen { doc, opts, out: String::new() };
    for id in ids {
        g.node(*id, 0, true);
    }
    g.out
}

// ---- Tailwind -----------------------------------------------------------------------------------

fn arb(v: &str) -> String {
    v.trim().replace(' ', "_")
}

/// Spacing scale: multiples of 4px map to `n` (Tailwind v4's 0.25rem steps); others are arbitrary.
fn spacing(v: &str) -> String {
    if let Some(name) = var_name(v).and_then(|n| n.strip_prefix("--spacing-")) {
        return name.to_string();
    }
    if let Some(n) = v.strip_suffix("px").and_then(|n| n.parse::<f64>().ok()) {
        if n == 0.0 {
            return "0".into();
        }
        let steps = n / 4.0;
        if (steps * 4.0).fract() == 0.0 && (steps * 2.0).fract() == 0.0 {
            return crate::style::fmt_num(steps);
        }
        if n == 1.0 {
            return "px".into();
        }
    }
    match v {
        "100%" => "full".into(),
        v if var_name(v).is_some() => format!("({})", var_name(v).unwrap()),
        v => format!("[{}]", arb(v)),
    }
}

fn color(v: &str) -> String {
    match var_name(v) {
        Some(n) if n.starts_with("--color-") => n["--color-".len()..].to_string(),
        Some(n) => format!("({n})"),
        None => format!("[{}]", arb(v)),
    }
}

fn neg(prefix: &str, v: &str) -> String {
    match v.strip_prefix('-') {
        Some(pos) if !pos.is_empty() => format!("-{prefix}-{}", spacing(pos)),
        _ => format!("{prefix}-{}", spacing(v)),
    }
}

/// Tailwind classes for one CSS declaration.
pub fn tailwind(prop: &str, v: &str) -> Vec<String> {
    let one = |s: String| vec![s];
    match (prop, v) {
        ("display", "none") => one("hidden".into()),
        ("display", d @ ("flex" | "block" | "grid" | "inline-flex" | "inline-block" | "inline" | "contents")) => {
            one(d.into())
        }
        ("flex-direction", "row") => vec![],
        ("flex-direction", "column") => one("flex-col".into()),
        ("flex-direction", "row-reverse") => one("flex-row-reverse".into()),
        ("flex-direction", "column-reverse") => one("flex-col-reverse".into()),
        ("flex-wrap", "wrap") => one("flex-wrap".into()),
        ("flex-wrap", "nowrap") => one("flex-nowrap".into()),
        ("align-items" | "align-self", a) => {
            let p = if prop == "align-items" { "items" } else { "self" };
            let s = match a {
                "flex-start" | "start" => "start",
                "flex-end" | "end" => "end",
                "center" => "center",
                "stretch" => "stretch",
                "baseline" => "baseline",
                "auto" => "auto",
                other => return one(format!("[{prop}:{}]", arb(other))),
            };
            one(format!("{p}-{s}"))
        }
        ("justify-content", j) => {
            let s = match j {
                "flex-start" | "start" => "start",
                "flex-end" | "end" => "end",
                "center" => "center",
                "space-between" => "between",
                "space-around" => "around",
                "space-evenly" => "evenly",
                "stretch" => "stretch",
                other => return one(format!("[justify-content:{}]", arb(other))),
            };
            one(format!("justify-{s}"))
        }
        ("gap" | "row-gap" | "column-gap", g) => {
            let p = match prop {
                "gap" => "gap",
                "row-gap" => "gap-y",
                _ => "gap-x",
            };
            one(format!("{p}-{}", spacing(g)))
        }
        ("padding", p) => {
            let parts: Vec<&str> = p.split_whitespace().collect();
            match parts[..] {
                [a] => one(format!("p-{}", spacing(a))),
                [y, x] => vec![format!("py-{}", spacing(y)), format!("px-{}", spacing(x))],
                [t, x, b] => {
                    vec![format!("pt-{}", spacing(t)), format!("px-{}", spacing(x)), format!("pb-{}", spacing(b))]
                }
                [t, r, b, l] => vec![
                    format!("pt-{}", spacing(t)),
                    format!("pr-{}", spacing(r)),
                    format!("pb-{}", spacing(b)),
                    format!("pl-{}", spacing(l)),
                ],
                _ => one(format!("[padding:{}]", arb(p))),
            }
        }
        ("width" | "height" | "min-width" | "max-width" | "min-height" | "max-height", s) => {
            let p = match prop {
                "width" => "w",
                "height" => "h",
                "min-width" => "min-w",
                "max-width" => "max-w",
                "min-height" => "min-h",
                _ => "max-h",
            };
            let val = match s {
                "fit-content" => "fit".into(),
                "max-content" => "max".into(),
                "min-content" => "min".into(),
                "auto" => "auto".into(),
                "100%" => "full".into(),
                s => spacing(s),
            };
            one(format!("{p}-{val}"))
        }
        ("flex-grow", "1") => one("grow".into()),
        ("flex-grow", "0") => one("grow-0".into()),
        ("flex-shrink", "0") => one("shrink-0".into()),
        ("flex-shrink", "1") => one("shrink".into()),
        ("flex-basis", b) => one(format!("basis-{}", if b == "0px" || b == "0%" { "0".into() } else { spacing(b) })),
        ("position", p @ ("absolute" | "relative" | "fixed" | "sticky" | "static")) => one(p.into()),
        ("left" | "top" | "right" | "bottom", v) => one(neg(prop, v)),
        ("z-index", z) => one(match z.strip_prefix('-') {
            Some(n) => format!("-z-{n}"),
            None => format!("z-{z}"),
        }),
        ("overflow", o @ ("hidden" | "clip" | "visible" | "auto" | "scroll")) => one(format!("overflow-{o}")),
        ("opacity", o) => {
            let pct = o.parse::<f64>().map(|f| f * 100.0).unwrap_or(-1.0);
            one(if pct >= 0.0 && pct.fract() == 0.0 { format!("opacity-{pct}") } else { format!("opacity-[{o}]") })
        }
        ("border-radius", r) => one(match r {
            "9999px" | "50%" => "rounded-full".into(),
            "0px" | "0" => "rounded-none".into(),
            r if r.contains(' ') => format!("rounded-[{}]", arb(r)),
            r if var_name(r).is_some_and(|n| n.starts_with("--radius-")) => {
                format!("rounded-{}", &var_name(r).unwrap()[9..])
            }
            r => format!("rounded-[{r}]"),
        }),
        ("background-color", c) => one(format!("bg-{}", color(c))),
        ("color", c) => one(format!("text-{}", color(c))),
        ("border", b) => {
            let w: Vec<&str> = split_top(b, ' ');
            let mut out = Vec::new();
            for part in w {
                match part {
                    "1px" => out.push("border".to_string()),
                    "solid" => {}
                    s @ ("dashed" | "dotted" | "double") => out.push(format!("border-{s}")),
                    p if p.ends_with("px") => out.push(format!("border-[{p}]")),
                    c => out.push(format!("border-{}", color(c))),
                }
            }
            out
        }
        ("border-width", w) => one(format!("[border-width:{}]", arb(w))),
        ("border-style", s) => one(format!("border-{s}")),
        ("border-color", c) => one(format!("border-{}", color(c))),
        ("box-shadow", s) => one(format!("shadow-[{}]", arb(s))),
        ("font-size", s) => one(match var_name(s).and_then(|n| n.strip_prefix("--text-")) {
            Some(n) => format!("text-{n}"),
            None => format!("text-[{}]", arb(s)),
        }),
        ("font-weight", w) => one(match w {
            "100" => "font-thin".into(),
            "200" => "font-extralight".into(),
            "300" => "font-light".into(),
            "400" => "font-normal".into(),
            "500" => "font-medium".into(),
            "600" => "font-semibold".into(),
            "700" => "font-bold".into(),
            "800" => "font-extrabold".into(),
            "900" => "font-black".into(),
            w => format!("font-[{w}]"),
        }),
        ("font-family", f) => one(match var_name(f).and_then(|n| n.strip_prefix("--font-")) {
            Some(n) => format!("font-{n}"),
            None => format!("font-[{}]", arb(&f.replace('"', "'"))),
        }),
        ("font-style", "italic") => one("italic".into()),
        ("line-height", l) => one(format!("leading-[{}]", arb(l))),
        ("letter-spacing", l) => one(format!("tracking-[{}]", arb(l))),
        ("text-align", a @ ("left" | "center" | "right" | "justify" | "start" | "end")) => one(format!("text-{a}")),
        ("text-transform", t @ ("uppercase" | "lowercase" | "capitalize")) => one(t.into()),
        ("text-transform", "none") => one("normal-case".into()),
        ("text-decoration", "underline") => one("underline".into()),
        ("text-decoration", "line-through") => one("line-through".into()),
        ("text-decoration", "overline") => one("overline".into()),
        ("white-space", "pre-wrap") => one("whitespace-pre-wrap".into()),
        ("white-space", w @ ("nowrap" | "pre" | "pre-line" | "normal")) => one(format!("whitespace-{w}")),
        ("rotate", r) => {
            one(if let Some(neg) = r.strip_prefix('-') { format!("-rotate-[{neg}]") } else { format!("rotate-[{r}]") })
        }
        ("mix-blend-mode", m) => one(format!("mix-blend-{m}")),
        ("filter", f) if f.starts_with("blur(") => one(format!("blur-[{}]", &f[5..f.len() - 1])),
        ("backdrop-filter", f) if f.starts_with("blur(") => one(format!("backdrop-blur-[{}]", &f[5..f.len() - 1])),
        (p, v) => one(format!("[{p}:{}]", arb(v))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        doc::Op,
        html::{ImageSrc, import},
    };

    fn doc_with(html: &str) -> (Document, Vec<NodeId>) {
        let mut doc = Document::new("Code");
        let page = doc.pages[0].id;
        let imp = import(&mut doc, html, Parent::Page(page), None, &mut |_| ImageSrc::Failed("no images".into()));
        for op in imp.ops {
            doc.apply(op).unwrap();
        }
        (doc, imp.roots)
    }

    fn opts(format: Format) -> CodeOpts<'static> {
        CodeOpts { format, include_ids: false, root_positioning: false, asset_url: &|h| format!("/assets/{h}") }
    }

    const CARD: &str = r##"<div layer-name="Card" style="display:flex; flex-direction:column; gap:16px; padding:24px; background:#ffffff; border-radius:12px; width:320px">
        <h2 style="font-size:20px; color:#111111; margin:0">Pricing</h2>
        <p style="font-size:14px; line-height:1.5; color:#555">Simple plans for <b>every</b> team.</p>
        <div style="display:flex; gap:8px"><span style="padding:8px 12px; background:#000; color:#fff">Start</span></div>
    </div>"##;

    #[test]
    fn import_builds_frames_and_text() {
        let (doc, roots) = doc_with(CARD);
        assert_eq!(roots.len(), 1);
        let card = doc.node(roots[0]).unwrap();
        assert_eq!(card.name, "Card");
        assert_eq!(card.kind, NodeKind::Frame);
        let kids: Vec<_> = doc.children(Parent::Node(roots[0])).map(|c| doc.node(c).unwrap()).collect();
        assert_eq!(kids.len(), 3);
        assert_eq!(kids[0].kind, NodeKind::Text);
        assert_eq!(kids[0].text.as_deref(), Some("Pricing"));
        assert_eq!(kids[0].style.font_weight, Some(700), "heading defaults");
        assert_eq!(kids[1].text.as_deref(), Some("Simple plans for every team."));
        assert_eq!(kids[2].kind, NodeKind::Frame);
    }

    #[test]
    fn themes_round_trip_as_data_theme() {
        let (doc, roots) = doc_with(r#"<div data-theme="dark"><div data-theme="Bad">x</div></div>"#);
        let html = code(&doc, &roots, &opts(Format::Html));
        assert!(html.starts_with(r#"<div data-theme="dark""#) && html.matches("data-theme").count() == 1, "{html}");
    }

    #[test]
    fn html_export_round_trips() {
        let (doc, roots) = doc_with(CARD);
        let html = code(&doc, &roots, &opts(Format::Html));
        let (doc2, roots2) = doc_with(&html);
        assert_eq!(code(&doc2, &roots2, &opts(Format::Html)), html);
    }

    #[test]
    fn jsx_inline_and_tailwind() {
        let (doc, roots) = doc_with(CARD);
        let jsx = code(&doc, &roots, &opts(Format::Jsx));
        assert!(
            jsx.starts_with(
                "<div style={{ width: \"320px\", display: \"flex\", flexDirection: \"column\", gap: \"16px\""
            ),
            "{jsx}"
        );
        assert!(jsx.contains(">Pricing</div>"));
        let tw = code(&doc, &roots, &opts(Format::Tailwind));
        assert!(tw.contains("className=\"w-80 flex flex-col gap-4 p-6 rounded-[12px] bg-[#ffffff]\""), "{tw}");
        assert!(tw.contains("px-3") && tw.contains("py-2"), "{tw}");
        assert!(tw.contains("text-[14px]") && tw.contains("leading-[1.5]"));
    }

    #[test]
    fn root_positioning_and_ids() {
        let (doc, roots) =
            doc_with(r#"<div style="position:absolute; left:10px; top:20px; width:100px; height:50px"></div>"#);
        let without = code(&doc, &roots, &opts(Format::Html));
        assert!(!without.contains("left"));
        let o = CodeOpts { include_ids: true, root_positioning: true, ..opts(Format::Html) };
        let with = code(&doc, &roots, &o);
        assert!(with.contains("left: 10px") && with.contains(&format!("id=\"{}\"", roots[0])));
    }

    #[test]
    fn svg_import_and_jsx_attribute_names() {
        let (doc, roots) = doc_with(
            r##"<svg viewBox="0 0 24 24" width="24" height="24" fill="none"><path d="M0 0L24 24" stroke="#000" stroke-width="2"/></svg>"##,
        );
        let svg = doc.node(roots[0]).unwrap();
        assert_eq!(svg.kind, NodeKind::Svg);
        assert_eq!(svg.attrs.get("viewBox").map(String::as_str), Some("0 0 24 24"));
        let path = doc.children(Parent::Node(roots[0])).next().unwrap();
        assert_eq!(doc.node(path).unwrap().tag.as_deref(), Some("path"));
        let jsx = code(&doc, &roots, &opts(Format::Jsx));
        assert!(jsx.contains("strokeWidth=\"2\"") && jsx.contains("viewBox=\"0 0 24 24\""), "{jsx}");
    }

    #[test]
    fn images_resolve_through_the_caller() {
        let mut doc = Document::new("Img");
        let page = doc.pages[0].id;
        let imp = import(
            &mut doc,
            r#"<img src="data:image/png;base64,AAAA" style="width:200px; height:100px; object-fit:contain"><img src="spark-gen://agent?prompt=cat"><img src="missing.png">"#,
            Parent::Page(page),
            None,
            &mut |src| {
                if src.starts_with("data:") {
                    ImageSrc::Asset {
                        hash: "h1".into(),
                        mime: "image/png".into(),
                        bytes: Some(vec![1]),
                        width: Some(10.0),
                        height: Some(5.0),
                    }
                } else if src.starts_with("spark-gen:") {
                    ImageSrc::Job(serde_json::json!({"status": "pending"}))
                } else {
                    ImageSrc::Failed(format!("can't load {src}"))
                }
            },
        );
        assert!(imp.ops.iter().any(|o| matches!(o, Op::AddAsset { hash, .. } if hash == "h1")));
        assert_eq!(imp.notes, vec!["can't load missing.png".to_string()]);
        for op in imp.ops {
            doc.apply(op).unwrap();
        }
        let a = doc.node(imp.roots[0]).unwrap();
        assert!(
            matches!(&a.style.fills[0], crate::style::Fill::Image { asset, fit: crate::style::Fit::Contain, .. } if asset == "h1")
        );
        assert!(doc.node(imp.roots[1]).unwrap().image.is_some());
    }

    #[test]
    fn clone_and_line_breaks() {
        let (mut doc, roots) = doc_with(r#"<p>One<br>Two   lines</p>"#);
        assert_eq!(doc.node(roots[0]).unwrap().text.as_deref(), Some("One\nTwo lines"));
        let page = doc.pages[0].id;
        let html =
            format!(r##"<spark-clone node-id="{}" layer-name="Copy" style="color:#f00"></spark-clone>"##, roots[0]);
        let imp = import(&mut doc, &html, Parent::Page(page), None, &mut |_| ImageSrc::Failed(String::new()));
        for op in imp.ops {
            doc.apply(op).unwrap();
        }
        let copy = doc.node(imp.roots[0]).unwrap();
        assert_eq!(copy.name, "Copy");
        assert_eq!(copy.text.as_deref(), Some("One\nTwo lines"));
        assert!(copy.style.color.is_some());
        let jsx = code(&doc, &imp.roots, &opts(Format::Jsx));
        assert!(jsx.contains("{\"One\\nTwo lines\"}"), "{jsx}");
    }
}
