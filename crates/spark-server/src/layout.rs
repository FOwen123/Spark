//! Headless layout. The UI measures layout in the browser; when it hasn't (a file only ever
//! touched by the CLI, or a page nobody opened), this computes one with taffy so exports still
//! work. Text is measured with the same fonts the exporters use.

use spark_core::{
    Document, NodeId, NodeKind, Parent, Rect,
    style::{Len, Size as S, Style},
    svg::{text_props, wrap},
};
use taffy::prelude::*;

/// A non-percentage length in px (em and rem against a 16px root; token references count as 0).
fn px(l: &Len) -> f32 {
    match l {
        Len::Em(v) | Len::Rem(v) => *v as f32 * 16.0,
        Len::Px(v) | Len::Num(v) => *v as f32,
        Len::Pct(_) | Len::Var(_) => 0.0,
    }
}

fn lp(l: &Len) -> LengthPercentage {
    match l {
        Len::Pct(p) => LengthPercentage::percent(*p as f32 / 100.0),
        l => LengthPercentage::length(px(l)),
    }
}

fn lpa(l: &Option<Len>) -> LengthPercentageAuto {
    match l {
        None => LengthPercentageAuto::auto(),
        Some(Len::Pct(p)) => LengthPercentageAuto::percent(*p as f32 / 100.0),
        Some(l) => LengthPercentageAuto::length(px(l)),
    }
}

fn dim(l: &Len) -> Dimension {
    match l {
        Len::Pct(p) => Dimension::percent(*p as f32 / 100.0),
        l => Dimension::length(px(l)),
    }
}

fn align(v: &Option<String>) -> Option<AlignItems> {
    Some(match v.as_deref()? {
        "flex-start" | "start" => AlignItems::FLEX_START,
        "flex-end" | "end" => AlignItems::FLEX_END,
        "center" => AlignItems::CENTER,
        "baseline" => AlignItems::BASELINE,
        "stretch" => AlignItems::STRETCH,
        _ => return None,
    })
}

fn justify(v: &Option<String>) -> Option<JustifyContent> {
    Some(match v.as_deref()? {
        "flex-start" | "start" => JustifyContent::FLEX_START,
        "flex-end" | "end" => JustifyContent::FLEX_END,
        "center" => JustifyContent::CENTER,
        "space-between" => JustifyContent::SPACE_BETWEEN,
        "space-around" => JustifyContent::SPACE_AROUND,
        "space-evenly" => JustifyContent::SPACE_EVENLY,
        _ => return None,
    })
}

fn taffy_style(s: &Style, hidden: bool, parent: Option<&Style>, text: bool) -> taffy::Style {
    let mut t = taffy::Style {
        display: match s.display.as_deref() {
            _ if hidden => Display::None,
            Some("flex" | "inline-flex") => Display::Flex,
            Some("none") => Display::None,
            Some("grid") => Display::Grid,
            _ => Display::Block,
        },
        ..Default::default()
    };
    if matches!(s.position.as_deref(), Some("absolute" | "fixed")) || parent.is_none() {
        t.position = Position::Absolute;
    }
    t.inset = taffy::Rect { left: lpa(&s.left), right: lpa(&s.right), top: lpa(&s.top), bottom: lpa(&s.bottom) };
    let pflex = parent.is_some_and(|p| p.is_flex());
    let prow = parent.is_some_and(|p| p.is_row());
    for (axis, size) in [(true, &s.width), (false, &s.height)] {
        let d = match size {
            Some(S::Fixed(l)) => dim(l),
            Some(S::Fill) if pflex && prow == axis => {
                t.flex_grow = 1.0;
                t.flex_basis = Dimension::length(0.0);
                Dimension::auto()
            }
            Some(S::Fill) if pflex => {
                t.align_self = Some(AlignSelf::STRETCH);
                Dimension::auto()
            }
            Some(S::Fill) => Dimension::percent(1.0),
            Some(S::Hug) | None => Dimension::auto(),
        };
        if axis { t.size.width = d } else { t.size.height = d }
    }
    // Text and hug-width blocks shrink to their content instead of stretching.
    if (text || matches!(s.width, Some(S::Hug))) && !pflex && t.position == Position::Relative {
        t.align_self = Some(AlignSelf::FLEX_START);
    }
    t.min_size = taffy::Size { width: lpa(&s.min_width), height: lpa(&s.min_height) };
    t.max_size = taffy::Size { width: lpa(&s.max_width), height: lpa(&s.max_height) };
    if let Some([a, b, c, d]) = &s.padding {
        t.padding = taffy::Rect { top: lp(a), right: lp(b), bottom: lp(c), left: lp(d) };
    }
    if let Some(b) = &s.border {
        let w = |v: f64| LengthPercentage::length(v as f32);
        t.border = taffy::Rect { top: w(b.width[0]), right: w(b.width[1]), bottom: w(b.width[2]), left: w(b.width[3]) };
    }
    if let Some(g) = &s.gap {
        t.gap = taffy::Size { width: lp(g), height: lp(g) };
    }
    t.flex_direction = match s.flex_direction.as_deref() {
        Some("column") => FlexDirection::Column,
        Some("column-reverse") => FlexDirection::ColumnReverse,
        Some("row-reverse") => FlexDirection::RowReverse,
        _ => FlexDirection::Row,
    };
    if s.flex_wrap.as_deref() == Some("wrap") {
        t.flex_wrap = FlexWrap::Wrap;
    }
    if let Some(a) = align(&s.align_items) {
        t.align_items = a;
    }
    if let Some(j) = justify(&s.justify_content) {
        t.justify_content = j;
    }
    if let Some(a) = align(&s.align_self) {
        t.align_self = Some(a);
    }
    if let Some(g) = s.flex_grow {
        t.flex_grow = g as f32;
    }
    if let Some(sh) = s.flex_shrink {
        t.flex_shrink = sh as f32;
    }
    if let Some(b) = &s.flex_basis {
        t.flex_basis = dim(b);
    }
    t
}

/// Whether any node in the subtree lacks a measured rect.
fn needs_layout(doc: &Document, id: NodeId) -> bool {
    std::iter::once(id).chain(doc.descendants(Parent::Node(id))).any(|n| !doc.measured.contains_key(&n))
}

/// Computes layout for the artboard containing `id` if anything in it is unmeasured, and records
/// the results as measured rects. They're saved with the file so the next open skips this; the
/// UI's own measurements replace them whenever it lays the artboard out.
pub fn ensure(doc: &mut Document, id: NodeId) {
    let Some(board) = doc.artboard_of(id) else { return };
    if !needs_layout(doc, board) {
        return;
    }
    let mut tree: TaffyTree<NodeId> = TaffyTree::new();
    fn build(
        doc: &Document,
        tree: &mut TaffyTree<NodeId>,
        id: NodeId,
        parent: Option<&Style>,
        out: &mut Vec<(NodeId, taffy::NodeId)>,
    ) -> taffy::NodeId {
        let n = doc.node(id).unwrap();
        let text = n.kind == NodeKind::Text;
        let style = taffy_style(&n.style, n.hidden, parent, text);
        let node = if n.kind == NodeKind::Frame {
            let kids: Vec<taffy::NodeId> = doc
                .children(Parent::Node(id))
                .filter(|c| doc.node(*c).is_some())
                .map(|c| build(doc, tree, c, Some(&n.style), out))
                .collect();
            tree.new_with_children(style, &kids).unwrap()
        } else {
            tree.new_leaf_with_context(style, id).unwrap()
        };
        out.push((id, node));
        node
    }
    let mut nodes = Vec::new();
    let root = build(doc, &mut tree, board, None, &mut nodes);
    let measure = |inputs: taffy::LayoutInput, _: taffy::NodeId, ctx: Option<&mut NodeId>, style: &taffy::Style| {
        taffy::compute_leaf_layout(
            inputs,
            style,
            |_, _| 0.0,
            |known, avail| {
                let Some(id) = ctx.as_deref().copied() else { return taffy::Size::ZERO };
                let Some(n) = doc.node(id) else { return taffy::Size::ZERO };
                if n.kind != NodeKind::Text {
                    return taffy::Size::ZERO;
                }
                let p = text_props(doc, id);
                let ts = p.style();
                let max = known.width.map(f64::from).unwrap_or(match avail.width {
                    AvailableSpace::Definite(w) => w as f64,
                    AvailableSpace::MinContent => 0.0,
                    AvailableSpace::MaxContent => f64::INFINITY,
                });
                let lines = wrap(&p.text, if max <= 0.0 { 1.0 } else { max + 0.5 }, &ts, &crate::export::measure);
                let width = lines.iter().map(|l| crate::export::measure(l, &ts)).fold(0.0, f64::max);
                taffy::Size {
                    width: known.width.unwrap_or(width.ceil() as f32),
                    height: known.height.unwrap_or((lines.len() as f64 * p.line_height) as f32),
                }
            },
        )
    };
    if tree.compute_layout_with_measure(root, taffy::Size::MAX_CONTENT, measure).is_err() {
        return;
    }
    // World positions: the artboard sits at its left/top; children are relative to their parent.
    let n = doc.node(board).unwrap();
    let origin =
        (n.style.left.as_ref().and_then(Len::px).unwrap_or(0.0), n.style.top.as_ref().and_then(Len::px).unwrap_or(0.0));
    let lookup: std::collections::HashMap<NodeId, taffy::NodeId> = nodes.into_iter().collect();
    let mut stack = vec![(board, origin)];
    while let Some((id, (px, py))) = stack.pop() {
        let Some(l) = lookup.get(&id).and_then(|t| tree.layout(*t).ok()) else { continue };
        let (x, y) = if id == board { (px, py) } else { (px + l.location.x as f64, py + l.location.y as f64) };
        doc.set_measured(id, Rect { x, y, w: l.size.width as f64, h: l.size.height as f64 });
        for c in doc.children(Parent::Node(id)).collect::<Vec<_>>() {
            stack.push((c, (x, y)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use spark_core::html::{ImageSrc, import};

    #[test]
    fn lays_out_flex_and_text_without_a_browser() {
        let mut doc = Document::new("L");
        let page = doc.pages[0].id;
        let imp = import(
            &mut doc,
            r#"<div style="position:absolute;left:100px;top:50px;width:400px;display:flex;flex-direction:column;gap:10px;padding:20px">
                <div style="height:40px;width:100%"></div>
                <p style="font-size:20px;line-height:1.5">Spark lays out text when no browser measured it, wrapping to fit</p>
                <div style="display:flex;gap:8px"><div style="width:fill;height:30px"></div><div style="width:fill;height:30px"></div></div>
            </div>"#,
            Parent::Page(page),
            None,
            &mut |_| ImageSrc::Failed(String::new()),
        );
        for op in imp.ops {
            doc.apply(op).unwrap();
        }
        let board = imp.roots[0];
        ensure(&mut doc, board);
        let r = doc.measured[&board];
        assert_eq!((r.x, r.y, r.w), (100.0, 50.0, 400.0));
        let kids: Vec<NodeId> = doc.children(Parent::Node(board)).collect();
        let bar = doc.measured[&kids[0]];
        assert_eq!((bar.x, bar.y, bar.w, bar.h), (120.0, 70.0, 360.0, 40.0));
        let text = doc.measured[&kids[1]];
        assert_eq!(text.y, 120.0);
        assert!(text.h >= 60.0, "wrapped onto two or more 30px lines: {text:?}");
        let row: Vec<NodeId> = doc.children(Parent::Node(kids[2])).collect();
        let (a, b) = (doc.measured[&row[0]], doc.measured[&row[1]]);
        assert_eq!(a.w, b.w);
        assert_eq!(b.x - a.x, a.w + 8.0);
        assert!((r.h - (20.0 + 40.0 + 10.0 + text.h + 10.0 + 30.0 + 20.0)).abs() < 1.0, "hug height {r:?}");
    }
}
