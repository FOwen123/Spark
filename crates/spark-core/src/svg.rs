//! SVG export, generated in Rust from the document and its last measured layout. Frames and
//! rectangles become shapes with fills, borders, radius, shadows and clipping; text becomes
//! `<text>` lines wrapped with the caller's font measurements; SVG layers embed as-is.

use std::fmt::Write;

use crate::{
    color::Color,
    doc::{Document, Parent, Rect},
    id::NodeId,
    node::NodeKind,
    style::{Fill, Fit, Len, Size, Style},
    tokens::resolve_in,
};

/// The font a text run uses, for measuring.
pub struct TextStyle<'a> {
    pub family: &'a str,
    pub size: f64,
    pub weight: u16,
    pub italic: bool,
    pub letter_spacing: f64,
}

/// Image data for an asset hash: (mime type, bytes).
pub type AssetData = Option<(String, Vec<u8>)>;

pub struct SvgOpts<'a> {
    /// Width of a single line of text in px.
    pub measure: &'a dyn Fn(&str, &TextStyle) -> f64,
    /// Embedded image data (mime, bytes) for an asset hash.
    pub asset: &'a dyn Fn(&str) -> AssetData,
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn num(v: f64) -> String {
    crate::style::fmt_num(v)
}

struct Gen<'a> {
    doc: &'a Document,
    opts: &'a SvgOpts<'a>,
    origin: (f64, f64),
    defs: String,
    next: usize,
    /// The theme of the layer being drawn, from it or its nearest themed ancestor.
    theme: Option<String>,
}

/// A color as an SVG paint and opacity, resolving token references in the layer's theme.
fn paint(doc: &Document, theme: Option<&str>, c: &Color) -> (String, f64) {
    let c = match c {
        Color::Var(name) => resolve_in(&doc.tokens, name, theme).and_then(|v| v.parse().ok()).unwrap_or(Color::BLACK),
        c => c.clone(),
    };
    let [r, g, b, a] = c.to_srgb().unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let byte = |v: f64| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    (format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b)), a)
}

fn len_px(l: &Len, base: f64, font: f64) -> f64 {
    match l {
        Len::Px(v) | Len::Num(v) => *v,
        Len::Pct(p) => base * p / 100.0,
        Len::Em(e) => e * font,
        Len::Rem(r) => r * 16.0,
        Len::Var(_) => 0.0,
    }
}

impl Gen<'_> {
    fn id(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}{}", self.next)
    }

    fn rect(&self, id: NodeId) -> Option<Rect> {
        let r = self.doc.measured.get(&id)?;
        Some(Rect { x: r.x - self.origin.0, y: r.y - self.origin.1, ..*r })
    }

    /// A rounded-rectangle path or rect element attributes for the node's box.
    fn shape(&self, r: &Rect, s: &Style) -> String {
        let radii = s
            .radius
            .as_ref()
            .map(|c| c.clone().map(|l| len_px(&l, r.w.min(r.h), 16.0).min(r.w / 2.0).min(r.h / 2.0)))
            .unwrap_or([0.0; 4]);
        if radii.iter().all(|v| *v == radii[0]) {
            return format!(
                r#"<rect x="{}" y="{}" width="{}" height="{}" rx="{}""#,
                num(r.x),
                num(r.y),
                num(r.w),
                num(r.h),
                num(radii[0])
            );
        }
        let [tl, tr, br, bl] = radii;
        let (x, y, w, h) = (r.x, r.y, r.w, r.h);
        format!(
            r#"<path d="M{} {} H{} A{tr} {tr} 0 0 1 {} {} V{} A{br} {br} 0 0 1 {} {} H{} A{bl} {bl} 0 0 1 {} {} V{} A{tl} {tl} 0 0 1 {} {} Z""#,
            num(x + tl),
            num(y),
            num(x + w - tr),
            num(x + w),
            num(y + tr),
            num(y + h - br),
            num(x + w - br),
            num(y + h),
            num(x + bl),
            num(x),
            num(y + h - bl),
            num(y + tl),
            num(x + tl),
            num(y)
        )
    }

    fn gradient(&mut self, fill: &Fill) -> String {
        let id = self.id("g");
        let theme = self.theme.clone();
        let stops = |stops: &[crate::style::Stop], doc: &Document| {
            stops
                .iter()
                .map(|s| {
                    let (c, a) = paint(doc, theme.as_deref(), &s.color);
                    format!(r#"<stop offset="{}" stop-color="{c}" stop-opacity="{}"/>"#, num(s.pos), num(a))
                })
                .collect::<String>()
        };
        match fill {
            // SVG has no conic gradient; it falls back to a linear one at the same angle.
            Fill::Linear { angle, stops: st, .. } | Fill::Conic { angle, stops: st, .. } => {
                // CSS angles point "to" a direction: 0deg is up, 90deg is right.
                let t = angle.to_radians();
                let (dx, dy) = (t.sin() / 2.0, -t.cos() / 2.0);
                let _ = write!(
                    self.defs,
                    r#"<linearGradient id="{id}" x1="{}" y1="{}" x2="{}" y2="{}">{}</linearGradient>"#,
                    num(0.5 - dx),
                    num(0.5 - dy),
                    num(0.5 + dx),
                    num(0.5 + dy),
                    stops(st, self.doc)
                );
            }
            Fill::Radial { stops: st, .. } => {
                let _ = write!(
                    self.defs,
                    r#"<radialGradient id="{id}" cx="0.5" cy="0.5" r="0.71">{}</radialGradient>"#,
                    stops(st, self.doc)
                );
            }
            _ => {}
        }
        format!("url(#{id})")
    }

    fn fills(&mut self, out: &mut String, r: &Rect, s: &Style, shape: &str) {
        for f in s.fills.iter().rev().filter(|f| !f.hidden()) {
            match f {
                Fill::Solid { color, .. } => {
                    let (c, a) = paint(self.doc, self.theme.as_deref(), color);
                    let _ = write!(out, r#"{shape} fill="{c}" fill-opacity="{}"/>"#, num(a));
                }
                Fill::Linear { .. } | Fill::Radial { .. } | Fill::Conic { .. } => {
                    let g = self.gradient(f);
                    let _ = write!(out, r#"{shape} fill="{g}"/>"#);
                }
                Fill::Image { asset, fit, .. } => {
                    let Some((mime, bytes)) = (self.opts.asset)(asset) else { continue };
                    use base64::Engine;
                    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
                    let clip = self.id("c");
                    let _ = write!(self.defs, r#"<clipPath id="{clip}">{shape}/></clipPath>"#);
                    let par = match fit {
                        Fit::Cover => "xMidYMid slice",
                        Fit::Contain => "xMidYMid meet",
                        Fit::Fill | Fit::Tile => "none",
                    };
                    let _ = write!(
                        out,
                        r#"<image clip-path="url(#{clip})" x="{}" y="{}" width="{}" height="{}" preserveAspectRatio="{par}" href="data:{mime};base64,{data}"/>"#,
                        num(r.x),
                        num(r.y),
                        num(r.w),
                        num(r.h)
                    );
                }
            }
        }
    }

    fn border(&mut self, out: &mut String, r: &Rect, s: &Style) {
        let Some(b) = &s.border else { return };
        let (c, a) = paint(self.doc, self.theme.as_deref(), &b.color);
        let dash = match b.style.as_str() {
            "dashed" => r#" stroke-dasharray="6 4""#,
            "dotted" => r#" stroke-dasharray="1 3" stroke-linecap="round""#,
            _ => "",
        };
        let [t, rt, bt, l] = b.width;
        if t == rt && rt == bt && bt == l {
            if t <= 0.0 {
                return;
            }
            // CSS borders sit inside the box; an SVG stroke is centred on the path.
            let inner = Rect { x: r.x + t / 2.0, y: r.y + t / 2.0, w: (r.w - t).max(0.0), h: (r.h - t).max(0.0) };
            let shape = self.shape(&inner, s);
            let _ = write!(
                out,
                r#"{shape} fill="none" stroke="{c}" stroke-opacity="{}" stroke-width="{}"{dash}/>"#,
                num(a),
                num(t)
            );
            return;
        }
        for (w, x1, y1, x2, y2) in [
            (t, r.x, r.y + t / 2.0, r.right(), r.y + t / 2.0),
            (rt, r.right() - rt / 2.0, r.y, r.right() - rt / 2.0, r.bottom()),
            (bt, r.x, r.bottom() - bt / 2.0, r.right(), r.bottom() - bt / 2.0),
            (l, r.x + l / 2.0, r.y, r.x + l / 2.0, r.bottom()),
        ] {
            if w > 0.0 {
                let _ = write!(
                    out,
                    r#"<line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{c}" stroke-opacity="{}" stroke-width="{}"{dash}/>"#,
                    num(x1),
                    num(y1),
                    num(x2),
                    num(y2),
                    num(a),
                    num(w)
                );
            }
        }
    }

    fn shadows(&mut self, out: &mut String, r: &Rect, s: &Style) {
        for sh in s.shadows.iter().filter(|sh| !sh.hidden && !sh.inset) {
            // shortcut: inset shadows are not drawn in SVG export; add them if users need them.
            let id = self.id("f");
            let (c, a) = paint(self.doc, self.theme.as_deref(), &sh.color);
            let _ = write!(
                self.defs,
                r#"<filter id="{id}" x="-50%" y="-50%" width="200%" height="200%"><feGaussianBlur stdDeviation="{}"/></filter>"#,
                num(sh.blur / 2.0)
            );
            let grown = Rect {
                x: r.x + sh.x - sh.spread,
                y: r.y + sh.y - sh.spread,
                w: r.w + 2.0 * sh.spread,
                h: r.h + 2.0 * sh.spread,
            };
            let shape = self.shape(&grown, s);
            let _ = write!(out, r#"{shape} fill="{c}" fill-opacity="{}" filter="url(#{id})"/>"#, num(a));
        }
    }

    fn text(&mut self, out: &mut String, r: &Rect, id: NodeId) {
        let p = text_props(self.doc, id);
        let ts = p.style();
        // Wrap to the measured width (plus a little slack for rounding), honouring explicit breaks.
        let n = self.doc.node(id).unwrap();
        let max = r.w + 1.0;
        let wraps = !matches!(n.style.width, Some(Size::Hug) | None) || (self.opts.measure)(&p.text, &ts) > max;
        let lines = wrap(&p.text, if wraps { max } else { f64::INFINITY }, &ts, self.opts.measure);
        let (size, weight, italic, tracking, line_h, family, align, color, deco) = (
            p.size,
            p.weight,
            p.italic,
            p.letter_spacing,
            p.line_height,
            p.family_css.clone(),
            p.align.clone(),
            p.color.clone(),
            p.decoration.clone(),
        );
        let (anchor, x) = match align.as_str() {
            "center" => ("middle", r.x + r.w / 2.0),
            "right" | "end" => ("end", r.right()),
            _ => ("start", r.x),
        };
        let (c, a) = paint(self.doc, self.theme.as_deref(), &color);
        // Baseline: half-leading above, then the ascent (about 0.8 em for typical UI fonts).
        let first = r.y + (line_h - size) / 2.0 + size * 0.8;
        let _ = write!(
            out,
            r#"<text font-family="{}" font-size="{}" font-weight="{weight}"{} fill="{c}" fill-opacity="{}" text-anchor="{anchor}"{}{} xml:space="preserve">"#,
            esc(&family),
            num(size),
            if italic { r#" font-style="italic""# } else { "" },
            num(a),
            if tracking != 0.0 { format!(r#" letter-spacing="{}""#, num(tracking)) } else { String::new() },
            deco.map(|d| format!(r#" text-decoration="{}""#, esc(&d))).unwrap_or_default(),
        );
        for (i, l) in lines.iter().enumerate() {
            let _ = write!(out, r#"<tspan x="{}" y="{}">{}</tspan>"#, num(x), num(first + i as f64 * line_h), esc(l));
        }
        out.push_str("</text>");
    }

    fn vector(&self, out: &mut String, id: NodeId) {
        let Some(n) = self.doc.node(id).filter(|n| !n.hidden) else { return };
        let tag = n.tag.as_deref().unwrap_or("g");
        let attrs: String =
            n.attrs.iter().filter(|(k, _)| *k != "data-text").map(|(k, v)| format!(r#" {k}="{}""#, esc(v))).collect();
        let _ = write!(out, "<{tag}{attrs}>");
        if let Some(t) = n.attrs.get("data-text") {
            out.push_str(&esc(t));
        }
        for c in self.doc.children(Parent::Node(id)) {
            self.vector(out, c);
        }
        let _ = write!(out, "</{tag}>");
    }

    fn node(&mut self, out: &mut String, id: NodeId) {
        let theme = self.doc.node(id).and_then(|n| n.theme.clone());
        let outer = if theme.is_some() { std::mem::replace(&mut self.theme, theme) } else { self.theme.clone() };
        self.draw(out, id);
        self.theme = outer;
    }

    fn draw(&mut self, out: &mut String, id: NodeId) {
        let Some(n) = self.doc.node(id).filter(|n| !n.hidden) else { return };
        let Some(r) = self.rect(id) else { return };
        let s = &n.style;
        let mut g = String::from("<g");
        if let Some(o) = s.opacity {
            let _ = write!(g, r#" opacity="{}""#, num(o));
        }
        if let Some(b) = s.blur {
            let f = self.id("b");
            let _ = write!(
                self.defs,
                r#"<filter id="{f}" x="-20%" y="-20%" width="140%" height="140%"><feGaussianBlur stdDeviation="{}"/></filter>"#,
                num(b / 2.0)
            );
            let _ = write!(g, r#" filter="url(#{f})""#);
        }
        if let Some(rot) = s.rotate.filter(|r| *r != 0.0) {
            let _ = write!(g, r#" transform="rotate({} {} {})""#, num(rot), num(r.x + r.w / 2.0), num(r.y + r.h / 2.0));
        }
        g.push('>');
        out.push_str(&g);
        if n.kind == NodeKind::Svg {
            let attrs: String = n
                .attrs
                .iter()
                .filter(|(k, _)| !matches!(k.as_str(), "width" | "height" | "x" | "y" | "style" | "xmlns"))
                .map(|(k, v)| format!(r#" {k}="{}""#, esc(v)))
                .collect();
            let _ = write!(
                out,
                r#"<svg x="{}" y="{}" width="{}" height="{}"{attrs}>"#,
                num(r.x),
                num(r.y),
                num(r.w),
                num(r.h)
            );
            for c in self.doc.children(Parent::Node(id)) {
                self.vector(out, c);
            }
            out.push_str("</svg></g>");
            return;
        }
        let shape = self.shape(&r, s);
        self.shadows(out, &r, s);
        self.fills(out, &r, s, &shape);
        if n.kind == NodeKind::Text {
            self.text(out, &r, id);
        }
        let clip = matches!(s.overflow.as_deref(), Some("hidden" | "clip"));
        let kids: Vec<NodeId> = self.doc.children(Parent::Node(id)).collect();
        if !kids.is_empty() {
            if clip {
                let c = self.id("c");
                let _ = write!(self.defs, r#"<clipPath id="{c}">{shape}/></clipPath>"#);
                let _ = write!(out, r#"<g clip-path="url(#{c})">"#);
            }
            for k in kids {
                self.node(out, k);
            }
            if clip {
                out.push_str("</g>");
            }
        }
        self.border(out, &r, s);
        out.push_str("</g>");
    }
}

/// Text properties of a text node, with inheritance from its ancestors resolved.
pub struct TextProps {
    pub text: String,
    pub family_css: String,
    pub family: String,
    pub size: f64,
    pub weight: u16,
    pub italic: bool,
    pub line_height: f64,
    pub letter_spacing: f64,
    pub align: String,
    pub color: Color,
    pub decoration: Option<String>,
}

impl TextProps {
    pub fn style(&self) -> TextStyle<'_> {
        TextStyle {
            family: &self.family,
            size: self.size,
            weight: self.weight,
            italic: self.italic,
            letter_spacing: self.letter_spacing,
        }
    }
}

pub fn text_props(doc: &Document, id: NodeId) -> TextProps {
    let n = doc.node(id).expect("text node");
    let mut chain = vec![&n.style];
    chain.extend(doc.ancestors(id).iter().filter_map(|a| doc.node(*a)).map(|a| &a.style));
    let find = |f: &dyn Fn(&Style) -> bool| chain.iter().copied().find(|s| f(s));
    let size = find(&|s| s.font_size.is_some())
        .and_then(|s| s.font_size.as_ref())
        .map(|l| len_px(l, 16.0, 16.0))
        .unwrap_or(16.0);
    let family_css = find(&|s| s.font_family.is_some())
        .and_then(|s| s.font_family.clone())
        .unwrap_or_else(|| "Geist, sans-serif".into());
    let line_height = find(&|s| s.line_height.is_some())
        .and_then(|s| s.line_height.as_ref())
        .map(|l| match l {
            Len::Num(m) => m * size,
            l => len_px(l, size, size),
        })
        .unwrap_or(size * 1.2);
    let mut text = n.text.clone().unwrap_or_default();
    match find(&|s| s.text_transform.is_some()).and_then(|s| s.text_transform.as_deref()) {
        Some("uppercase") => text = text.to_uppercase(),
        Some("lowercase") => text = text.to_lowercase(),
        _ => {}
    }
    TextProps {
        text,
        family: family_css.split(',').next().unwrap_or("Geist").trim().trim_matches(['"', '\'']).to_string(),
        family_css,
        size,
        weight: find(&|s| s.font_weight.is_some()).and_then(|s| s.font_weight).unwrap_or(400),
        italic: find(&|s| s.font_style.is_some()).and_then(|s| s.font_style.as_deref()) == Some("italic"),
        line_height,
        letter_spacing: find(&|s| s.letter_spacing.is_some())
            .and_then(|s| s.letter_spacing.as_ref())
            .map(|l| len_px(l, size, size))
            .unwrap_or(0.0),
        align: find(&|s| s.text_align.is_some()).and_then(|s| s.text_align.clone()).unwrap_or_else(|| "left".into()),
        color: find(&|s| s.color.is_some()).and_then(|s| s.color.clone()).unwrap_or(Color::BLACK),
        decoration: find(&|s| s.text_decoration.is_some()).and_then(|s| s.text_decoration.clone()),
    }
}

/// Greedy word wrapping to `max` px, honouring explicit line breaks. Each word is measured once.
pub fn wrap(text: &str, max: f64, ts: &TextStyle, measure: &dyn Fn(&str, &TextStyle) -> f64) -> Vec<String> {
    let space = measure(" ", ts);
    let mut lines = Vec::new();
    for para in text.split('\n') {
        if !max.is_finite() {
            lines.push(para.to_string());
            continue;
        }
        let mut line = String::new();
        let mut width = 0.0;
        for word in para.split(' ') {
            let w = measure(word, ts);
            if !line.is_empty() && width + space + w > max {
                lines.push(std::mem::take(&mut line));
                width = 0.0;
            }
            if !line.is_empty() {
                line.push(' ');
                width += space;
            }
            line.push_str(word);
            width += w;
        }
        lines.push(line);
    }
    lines
}

/// A standalone SVG for one node (with its subtree), sized to its measured box.
pub fn node_svg(doc: &Document, id: NodeId, opts: &SvgOpts) -> Result<String, String> {
    doc.node(id).ok_or_else(|| format!("node {id} not found"))?;
    let r = *doc
        .measured
        .get(&id)
        .ok_or_else(|| format!("{id} has no measured size yet; open the file in Spark once so its layout is known"))?;
    // A layer inside a themed frame is drawn in that frame's theme.
    let theme = doc.ancestors(id).into_iter().find_map(|a| doc.node(a)?.theme.clone());
    let mut g = Gen { doc, opts, origin: (r.x, r.y), defs: String::new(), next: 0, theme };
    let mut body = String::new();
    g.node(&mut body, id);
    Ok(format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{w}" height="{h}" viewBox="0 0 {w} {h}"><defs>{}</defs>{body}</svg>"#,
        g.defs,
        w = num(r.w),
        h = num(r.h)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        doc::Op,
        html::{ImageSrc, import},
    };

    #[test]
    fn exports_shapes_text_and_clipping() {
        let mut doc = Document::new("Svg");
        let page = doc.pages[0].id;
        let imp = import(
            &mut doc,
            r##"<div style="width:200px;height:100px;background:linear-gradient(90deg,#000,#fff);border-radius:8px;border:2px dashed #ff0000;overflow:hidden;box-shadow:0 4px 8px rgba(0,0,0,.2)"><p style="font-size:20px;color:#111">Hello &amp; welcome to Spark</p></div>"##,
            Parent::Page(page),
            None,
            &mut |_| ImageSrc::Failed(String::new()),
        );
        for op in imp.ops {
            doc.apply(op).unwrap();
        }
        let root = imp.roots[0];
        let text = doc.children(Parent::Node(root)).next().unwrap();
        doc.set_measured(root, Rect { x: 50.0, y: 50.0, w: 200.0, h: 100.0 });
        doc.set_measured(text, Rect { x: 50.0, y: 50.0, w: 120.0, h: 48.0 });
        let measure = |t: &str, s: &TextStyle| t.chars().count() as f64 * s.size * 0.5;
        let svg = node_svg(&doc, root, &SvgOpts { measure: &measure, asset: &|_| None }).unwrap();
        assert!(
            svg.starts_with(r#"<svg xmlns="http://www.w3.org/2000/svg""#)
                && svg.contains(r#"width="200" height="100""#)
        );
        assert!(svg.contains("<linearGradient") && svg.contains(r#"rx="8""#));
        assert!(svg.contains(r##"stroke="#ff0000""##) && svg.contains("stroke-dasharray"));
        assert!(svg.contains("clip-path") && svg.contains("feGaussianBlur"));
        assert!(svg.contains("Hello &amp;"), "{svg}");
        assert!(svg.matches("<tspan").count() >= 2, "text wraps to the measured width: {svg}");
        assert!(node_svg(&doc, NodeId::new(999, 0), &SvgOpts { measure: &measure, asset: &|_| None }).is_err());
        let _ = Op::Delete { id: root };
    }

    #[test]
    fn themed_frames_draw_with_their_theme_values() {
        let mut doc = Document::new("Themes");
        let page = doc.pages[0].id;
        for t in crate::tokens::starter() {
            doc.apply(Op::SetToken { name: t.name.clone(), token: Some(t) }).unwrap();
        }
        let imp = import(
            &mut doc,
            r#"<div data-theme="dark" style="width:100px;height:100px"><div style="width:50px;height:50px;background:var(--color-background)"></div></div>"#,
            Parent::Page(page),
            None,
            &mut |_| ImageSrc::Failed(String::new()),
        );
        for op in imp.ops {
            doc.apply(op).unwrap();
        }
        let root = imp.roots[0];
        assert_eq!(doc.node(root).unwrap().theme.as_deref(), Some("dark"));
        let inner = doc.children(Parent::Node(root)).next().unwrap();
        doc.set_measured(root, Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 });
        doc.set_measured(inner, Rect { x: 0.0, y: 0.0, w: 50.0, h: 50.0 });
        let measure = |_: &str, _: &TextStyle| 0.0;
        let o = SvgOpts { measure: &measure, asset: &|_| None };
        // The dark background is oklch(0.145 0 0), #0a0a0a; the base one is white.
        assert!(node_svg(&doc, root, &o).unwrap().contains(r##"fill="#0a0a0a""##));
        assert!(node_svg(&doc, inner, &o).unwrap().contains(r##"fill="#0a0a0a""##), "theme comes from the ancestor");
        doc.apply(Op::Set { id: root, prop: "theme".into(), value: serde_json::Value::Null }).unwrap();
        assert!(node_svg(&doc, inner, &o).unwrap().contains(r##"fill="#ffffff""##));
    }
}
