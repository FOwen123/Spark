//! Geometry-aware edits: drag and resize sessions with snapping (R-tree), nudging, duplicating,
//! wrapping in frames, flex layout, z-order and reparenting.

use rstar::{AABB, RTree, primitives::GeomWithData, primitives::Rectangle};
use serde_json::{Value, json};
use spark_core::{
    Document, Node, NodeId, NodeKind, Op, Parent, Rect,
    style::{Len, Size},
};

use crate::hub::OpenDoc;

pub fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

fn px(v: f64) -> Value {
    json!(format!("{}px", round2(v)))
}

/// World position of a parent's content origin (inside its border), from measured layout.
pub fn origin_of(doc: &Document, parent: Parent) -> (f64, f64) {
    match parent {
        Parent::Page(_) => (0.0, 0.0),
        Parent::Node(p) => {
            let r = doc.measured.get(&p).copied().unwrap_or_default();
            let b = doc
                .node(p)
                .and_then(|n| n.style.border.as_ref())
                .map(|b| (b.width[3], b.width[0]))
                .unwrap_or((0.0, 0.0));
            (r.x + b.0, r.y + b.1)
        }
    }
}

/// Whether a node is placed by left/top (freeform) rather than by its parent's flow.
pub fn is_free(doc: &Document, id: NodeId) -> bool {
    let Some(n) = doc.node(id) else { return false };
    matches!(doc.parent(id), Some(Parent::Page(_))) || matches!(n.style.position.as_deref(), Some("absolute" | "fixed"))
}

fn parent_is_flex(doc: &Document, parent: Parent) -> bool {
    matches!(parent, Parent::Node(p) if doc.node(p).is_some_and(|n| n.style.is_flex()))
}

/// Ops that place a node's top-left corner at world (x, y) under `parent`.
pub fn place_ops(doc: &Document, id: NodeId, parent: Parent, x: f64, y: f64) -> Vec<Op> {
    let (ox, oy) = origin_of(doc, parent);
    let mut ops = vec![
        Op::Set { id, prop: "style.left".into(), value: px(x - ox) },
        Op::Set { id, prop: "style.top".into(), value: px(y - oy) },
    ];
    let n = doc.node(id).unwrap();
    if n.style.position.as_deref() != Some("absolute") {
        ops.push(Op::Set { id, prop: "style.position".into(), value: json!("absolute") });
    }
    if n.style.right.is_some() {
        ops.push(Op::Set { id, prop: "style.right".into(), value: Value::Null });
    }
    if n.style.bottom.is_some() {
        ops.push(Op::Set { id, prop: "style.bottom".into(), value: Value::Null });
    }
    ops
}

type Entry = GeomWithData<Rectangle<[f64; 2]>, NodeId>;

fn entry(id: NodeId, r: &Rect) -> Entry {
    GeomWithData::new(Rectangle::from_corners([r.x, r.y], [r.right(), r.bottom()]), id)
}

/// A guide line in world coordinates.
#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct Guide {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub dashed: bool,
}

/// Snapping targets around a moving selection: siblings and the parent, in an R-tree.
pub struct Snapper {
    tree: RTree<Entry>,
}

impl Snapper {
    pub fn new(doc: &Document, moving: &[NodeId]) -> Snapper {
        let mut entries = Vec::new();
        let parents: Vec<Parent> = moving.iter().filter_map(|id| doc.parent(*id)).collect();
        for parent in parents.iter().copied().collect::<std::collections::HashSet<_>>() {
            for sib in doc.children(parent) {
                if moving.contains(&sib) || doc.node(sib).is_none_or(|n| n.hidden) {
                    continue;
                }
                if let Some(r) = doc.measured.get(&sib) {
                    entries.push(entry(sib, r));
                }
            }
            if let Parent::Node(p) = parent
                && let Some(r) = doc.measured.get(&p)
            {
                entries.push(entry(p, r));
            }
        }
        Snapper { tree: RTree::bulk_load(entries) }
    }

    fn near(&self, r: &Rect, reach: f64) -> Vec<Rect> {
        let env = AABB::from_corners([r.x - reach, r.y - reach], [r.right() + reach, r.bottom() + reach]);
        self.tree
            .locate_in_envelope_intersecting(env)
            .map(|e| {
                let (lo, hi) = (e.geom().lower(), e.geom().upper());
                Rect { x: lo[0], y: lo[1], w: hi[0] - lo[0], h: hi[1] - lo[1] }
            })
            .collect()
    }

    /// Snaps a moving rect's edges and centers to nearby rects within `threshold` world units.
    /// Returns the offset to add and the guides to draw.
    pub fn snap_move(&self, r: &Rect, threshold: f64) -> (f64, f64, Vec<Guide>) {
        let cands = self.near(r, 3000.0);
        let xs = |q: &Rect| [q.x, q.x + q.w / 2.0, q.right()];
        let ys = |q: &Rect| [q.y, q.y + q.h / 2.0, q.bottom()];
        let best = |mine: [f64; 3], theirs: &dyn Fn(&Rect) -> [f64; 3]| {
            let mut best: Option<f64> = None;
            for c in &cands {
                for a in mine {
                    for b in theirs(c) {
                        let d = b - a;
                        if d.abs() <= threshold && best.is_none_or(|bd| d.abs() < bd.abs()) {
                            best = Some(d);
                        }
                    }
                }
            }
            best
        };
        let mut dx = best(xs(r), &xs);
        let mut dy = best(ys(r), &ys);
        let mut guides = Vec::new();
        // Equal spacing between the nearest neighbors on each side.
        let between = |horizontal: bool, snapped: Option<f64>| -> Option<(f64, Vec<Guide>)> {
            let (lo, len, cross_lo, cross_hi) =
                if horizontal { (r.x, r.w, r.y, r.bottom()) } else { (r.y, r.h, r.x, r.right()) };
            let overlaps = |c: &Rect| {
                if horizontal {
                    c.y < cross_hi && c.bottom() > cross_lo
                } else {
                    c.x < cross_hi && c.right() > cross_lo
                }
            };
            let start = |c: &Rect| if horizontal { c.x } else { c.y };
            let end = |c: &Rect| if horizontal { c.right() } else { c.bottom() };
            let before = cands
                .iter()
                .filter(|c| overlaps(c) && end(c) <= lo + threshold)
                .max_by(|a, b| end(a).total_cmp(&end(b)))?;
            let after = cands
                .iter()
                .filter(|c| overlaps(c) && start(c) >= lo + len - threshold)
                .min_by(|a, b| start(a).total_cmp(&start(b)))?;
            let gap = (start(after) - end(before) - len) / 2.0;
            if gap < 0.0 {
                return None;
            }
            let d = end(before) + gap - lo;
            if d.abs() > threshold || snapped.is_some_and(|s| s.abs() < d.abs()) {
                return None;
            }
            let mid = (cross_lo + cross_hi) / 2.0;
            let seg = |a: f64, b: f64| {
                if horizontal {
                    Guide { x1: a, y1: mid, x2: b, y2: mid, dashed: true }
                } else {
                    Guide { x1: mid, y1: a, x2: mid, y2: b, dashed: true }
                }
            };
            Some((d, vec![seg(end(before), end(before) + gap), seg(end(before) + gap + len, start(after))]))
        };
        if let Some((d, g)) = between(true, dx) {
            dx = Some(d);
            guides.extend(g);
        }
        if let Some((d, g)) = between(false, dy) {
            dy = Some(d);
            guides.extend(g);
        }
        let moved = Rect { x: r.x + dx.unwrap_or(0.0), y: r.y + dy.unwrap_or(0.0), ..*r };
        guides.extend(self.align_guides(&moved, &cands, dx.is_some(), dy.is_some()));
        (dx.unwrap_or(0.0), dy.unwrap_or(0.0), guides)
    }

    /// Solid guides through every edge or center the rect shares with a candidate.
    fn align_guides(&self, r: &Rect, cands: &[Rect], x: bool, y: bool) -> Vec<Guide> {
        let mut out = Vec::new();
        let eq = |a: f64, b: f64| (a - b).abs() < 0.5;
        if x {
            for v in [r.x, r.x + r.w / 2.0, r.right()] {
                let hits: Vec<&Rect> =
                    cands.iter().filter(|c| [c.x, c.x + c.w / 2.0, c.right()].iter().any(|cv| eq(*cv, v))).collect();
                if !hits.is_empty() {
                    let y1 = hits.iter().map(|c| c.y).fold(r.y, f64::min);
                    let y2 = hits.iter().map(|c| c.bottom()).fold(r.bottom(), f64::max);
                    out.push(Guide { x1: v, y1, x2: v, y2, dashed: false });
                }
            }
        }
        if y {
            for v in [r.y, r.y + r.h / 2.0, r.bottom()] {
                let hits: Vec<&Rect> =
                    cands.iter().filter(|c| [c.y, c.y + c.h / 2.0, c.bottom()].iter().any(|cv| eq(*cv, v))).collect();
                if !hits.is_empty() {
                    let x1 = hits.iter().map(|c| c.x).fold(r.x, f64::min);
                    let x2 = hits.iter().map(|c| c.right()).fold(r.right(), f64::max);
                    out.push(Guide { x1, y1: v, x2, y2: v, dashed: false });
                }
            }
        }
        out
    }

    /// Snaps the edges of a resized rect that moved away from `from`.
    pub fn snap_resize(&self, from: &Rect, r: &Rect, threshold: f64) -> (Rect, Vec<Guide>) {
        let cands = self.near(r, 3000.0);
        let snap1 = |v: f64, vals: &dyn Fn(&Rect) -> [f64; 3]| -> Option<f64> {
            cands
                .iter()
                .flat_map(vals)
                .filter(|t| (t - v).abs() <= threshold)
                .min_by(|a, b| (a - v).abs().total_cmp(&(b - v).abs()))
        };
        let xs = |q: &Rect| [q.x, q.x + q.w / 2.0, q.right()];
        let ys = |q: &Rect| [q.y, q.y + q.h / 2.0, q.bottom()];
        let (mut x1, mut y1, mut x2, mut y2) = (r.x, r.y, r.right(), r.bottom());
        let mut guides = Vec::new();
        let moved = |a: f64, b: f64| (a - b).abs() > 1e-6;
        if moved(x1, from.x)
            && let Some(v) = snap1(x1, &xs)
        {
            x1 = v;
            guides.push(Guide { x1: v, y1: y1.min(from.y), x2: v, y2: y2.max(from.bottom()), dashed: false });
        }
        if moved(x2, from.right())
            && let Some(v) = snap1(x2, &xs)
        {
            x2 = v;
            guides.push(Guide { x1: v, y1: y1.min(from.y), x2: v, y2: y2.max(from.bottom()), dashed: false });
        }
        if moved(y1, from.y)
            && let Some(v) = snap1(y1, &ys)
        {
            y1 = v;
            guides.push(Guide { x1: x1.min(from.x), y1: v, x2: x2.max(from.right()), y2: v, dashed: false });
        }
        if moved(y2, from.bottom())
            && let Some(v) = snap1(y2, &ys)
        {
            y2 = v;
            guides.push(Guide { x1: x1.min(from.x), y1: v, x2: x2.max(from.right()), y2: v, dashed: false });
        }
        (Rect { x: x1, y: y1, w: (x2 - x1).max(1.0), h: (y2 - y1).max(1.0) }, guides)
    }
}

/// An in-progress drag or resize, grouped into one undo step.
pub struct Session {
    pub ids: Vec<NodeId>,
    origins: Vec<(NodeId, Rect)>,
    union: Rect,
    snapper: Snapper,
}

fn union(rects: impl IntoIterator<Item = Rect>) -> Option<Rect> {
    rects.into_iter().reduce(|a, b| {
        let (x, y) = (a.x.min(b.x), a.y.min(b.y));
        Rect { x, y, w: a.right().max(b.right()) - x, h: a.bottom().max(b.bottom()) - y }
    })
}

/// Top-most nodes only (drops nodes whose ancestor is also in the list), measured and editable.
fn roots(doc: &Document, ids: &[NodeId]) -> Vec<NodeId> {
    ids.iter()
        .copied()
        .filter(|id| doc.node(*id).is_some_and(|n| !n.locked) && !doc.ancestors(*id).iter().any(|a| ids.contains(a)))
        .collect()
}

/// Drops layers another client deleted since the session started.
fn prune_session(open: &mut OpenDoc) {
    let doc = &open.editor.doc;
    if let Some(s) = &mut open.session {
        s.ids.retain(|id| doc.is_live(*id));
        s.origins.retain(|(id, _)| doc.is_live(*id));
    }
}

pub fn start_session(open: &mut OpenDoc, ids: &[NodeId]) -> Result<(), String> {
    let doc = &open.editor.doc;
    let ids = roots(doc, ids);
    let origins: Vec<(NodeId, Rect)> = ids.iter().filter_map(|id| Some((*id, *doc.measured.get(id)?))).collect();
    let union = union(origins.iter().map(|(_, r)| *r)).ok_or("nothing measured to move")?;
    let snapper = Snapper::new(doc, &ids);
    open.editor.begin();
    open.session = Some(Session { ids, origins, union, snapper });
    Ok(())
}

/// Deep-copies `id` and its subtree under `parent` at `key`. Returns the new root id, the ops,
/// and (old, new) id pairs for every copied node.
pub fn clone_with_map(
    doc: &mut Document,
    id: NodeId,
    parent: Parent,
    key: String,
) -> (NodeId, Vec<Op>, Vec<(NodeId, NodeId)>) {
    let mut ops = Vec::new();
    let mut pairs = Vec::new();
    let mut stack = vec![(id, parent, key)];
    let mut root = None;
    while let Some((src, parent, key)) = stack.pop() {
        let Some(mut node) = doc.node(src).cloned() else { continue };
        node.id = doc.alloc_node();
        pairs.push((src, node.id));
        root.get_or_insert(node.id);
        for (k, child) in doc.child_keys(Parent::Node(src)).to_vec() {
            stack.push((child, Parent::Node(node.id), k));
        }
        if let Some(r) = doc.measured.get(&src).copied() {
            doc.measured.insert(node.id, r);
        }
        ops.push(Op::Insert { node, parent, key });
    }
    (root.unwrap(), ops, pairs)
}

pub fn clone_ops(doc: &mut Document, id: NodeId, parent: Parent, key: String) -> (NodeId, Vec<Op>) {
    let (root, ops, _) = clone_with_map(doc, id, parent, key);
    (root, ops)
}

/// The key just after `id` among its siblings.
fn key_after(doc: &Document, id: NodeId) -> (Parent, String) {
    let parent = doc.parent(id).unwrap();
    let index = doc.children(parent).position(|c| c == id).map(|i| i + 1);
    (parent, doc.key_at(parent, index, None))
}

/// Free space to the right of every artboard on the page, for auto-placed artboards.
pub fn free_spot(doc: &Document, page: spark_core::PageId) -> (f64, f64) {
    let boards: Vec<Rect> = doc.children(Parent::Page(page)).filter_map(|id| doc.measured.get(&id).copied()).collect();
    match union(boards.iter().copied()) {
        None => (0.0, 0.0),
        Some(u) => (round2(u.right() + 80.0), round2(u.y)),
    }
}

/// Copies nodes next to the originals. With `auto_place`, artboard copies go to free space.
pub fn duplicate(open: &mut OpenDoc, ids: &[NodeId], auto_place: bool) -> Result<Duplicated, String> {
    let doc = &mut open.editor.doc;
    let ids = roots(doc, ids);
    let mut ops = Vec::new();
    let mut new_ids = Vec::new();
    let mut pairs = Vec::new();
    let mut placed: Vec<Rect> = Vec::new();
    for id in ids {
        let (parent, key) = key_after(doc, id);
        let (nid, mut o, p) = clone_with_map(doc, id, parent, key);
        pairs.extend(p);
        if let (Parent::Page(page), true) = (parent, auto_place) {
            // Artboards land in free space to the right instead of on top of the original.
            if let Some(r) = doc.measured.get(&id).copied() {
                let (x, _) = free_spot(doc, page);
                let x = placed.iter().map(|p| p.right() + 80.0).fold(x, f64::max);
                placed.push(Rect { x, ..r });
                if let Some(Op::Insert { node, .. }) = o.first_mut() {
                    node.style.left = Some(Len::Px(x));
                    node.style.top = Some(Len::Px(round2(r.y)));
                }
                doc.measured.insert(nid, Rect { x, ..r });
            }
        }
        ops.append(&mut o);
        new_ids.push(nid);
    }
    Ok((new_ids, ops, pairs))
}

/// New root ids, the insert ops, and (old, new) pairs for every copied node.
pub type Duplicated = (Vec<NodeId>, Vec<Op>, Vec<(NodeId, NodeId)>);

pub fn drag(
    open: &mut OpenDoc,
    dx: f64,
    dy: f64,
    point: Option<(f64, f64)>,
    snap: bool,
    zoom: f64,
) -> Result<(Value, Vec<Op>), String> {
    prune_session(open);
    let Some(s) = &open.session else { return Err("no drag in progress".into()) };
    let moved = Rect { x: s.union.x + dx, y: s.union.y + dy, ..s.union };
    let (sx, sy, guides) =
        if snap { s.snapper.snap_move(&moved, 4.0 / zoom.max(0.01)) } else { (0.0, 0.0, Vec::new()) };
    let (dx, dy) = (dx + sx, dy + sy);
    let doc = &open.editor.doc;
    let mut ops = Vec::new();
    for (id, r) in &s.origins {
        if is_free(doc, *id) {
            let parent = doc.parent(*id).unwrap();
            ops.extend(
                place_ops(doc, *id, parent, r.x + dx, r.y + dy)
                    .into_iter()
                    .filter(|op| !matches!(op, Op::Set { prop, .. } if prop == "style.position")),
            );
        } else if let (Some(parent), Some((x, y))) = (doc.parent(*id), point) {
            // Flow children reorder live inside their flex parent.
            if parent_is_flex(doc, parent) {
                let row = matches!(parent, Parent::Node(p) if doc.node(p).unwrap().style.is_row());
                let pos = if row { x } else { y };
                let index = doc
                    .children(parent)
                    .filter(|c| c != id)
                    .filter(|c| {
                        doc.measured
                            .get(c)
                            .is_some_and(|r| if row { r.x + r.w / 2.0 < pos } else { r.y + r.h / 2.0 < pos })
                    })
                    .count();
                let now = doc.children(parent).position(|c| c == *id).unwrap_or(0);
                if index != now {
                    ops.push(Op::Move { id: *id, parent, key: doc.key_at(parent, Some(index), Some(*id)) });
                }
            }
        }
    }
    let ops = open.editor.transact(ops, None).map_err(|e| e.to_string())?;
    Ok((json!({"guides": guides, "dx": dx, "dy": dy}), ops))
}

/// The deepest visible, unlocked frame containing world point (x, y), skipping `exclude` subtrees.
pub fn frame_at(doc: &Document, page: spark_core::PageId, x: f64, y: f64, exclude: &[NodeId]) -> Parent {
    let mut best: Option<(usize, NodeId)> = None;
    for id in doc.descendants(Parent::Page(page)) {
        let Some(n) = doc.node(id) else { continue };
        if n.kind != NodeKind::Frame || n.hidden || n.locked {
            continue;
        }
        let anc = doc.ancestors(id);
        if exclude.contains(&id) || anc.iter().any(|a| exclude.contains(a)) {
            continue;
        }
        if anc.iter().any(|a| doc.node(*a).is_some_and(|n| n.hidden)) {
            continue;
        }
        if let Some(r) = doc.measured.get(&id)
            && x >= r.x
            && x <= r.right()
            && y >= r.y
            && y <= r.bottom()
            && best.is_none_or(|(d, _)| anc.len() >= d)
        {
            best = Some((anc.len(), id));
        }
    }
    best.map(|(_, id)| Parent::Node(id)).unwrap_or(Parent::Page(page))
}

/// Moves `ids` under `target`, keeping their world position when the target is freeform and
/// ordering them by position when it is a flex container.
pub fn reparent_ops(
    doc: &Document,
    ids: &[NodeId],
    target: Parent,
    index: Option<usize>,
    world: &[(NodeId, Rect)],
) -> Vec<Op> {
    let flex = parent_is_flex(doc, target);
    let mut ops = Vec::new();
    let mut tmp_index = index;
    for id in ids {
        let key = doc.key_at(target, tmp_index, Some(*id));
        tmp_index = tmp_index.map(|i| i + 1);
        ops.push(Op::Move { id: *id, parent: target, key });
        let r = world.iter().find(|(w, _)| w == id).map(|(_, r)| *r).or_else(|| doc.measured.get(id).copied());
        if flex {
            for f in ["position", "left", "top", "right", "bottom"] {
                if !doc.node(*id).unwrap().style.get_field(f).is_null() {
                    ops.push(Op::Set { id: *id, prop: format!("style.{f}"), value: Value::Null });
                }
            }
        } else if let Some(r) = r {
            let (ox, oy) = origin_of(doc, target);
            ops.push(Op::Set { id: *id, prop: "style.position".into(), value: json!("absolute") });
            ops.push(Op::Set { id: *id, prop: "style.left".into(), value: px(r.x - ox) });
            ops.push(Op::Set { id: *id, prop: "style.top".into(), value: px(r.y - oy) });
        }
    }
    // Insert ops for multiple ids: later keys must follow earlier ones, so recompute after each.
    ops
}

pub fn end_drag(open: &mut OpenDoc, point: Option<(f64, f64)>, reparent: bool) -> Result<Vec<Op>, String> {
    prune_session(open);
    let Some(s) = open.session.take() else { return Ok(vec![]) };
    let mut ops = Vec::new();
    if let (true, Some((x, y)), Some(page)) = (reparent, point, open.editor.page) {
        let doc = &open.editor.doc;
        let free: Vec<NodeId> = s.ids.iter().copied().filter(|id| is_free(doc, *id)).collect();
        if !free.is_empty() {
            let target = frame_at(doc, page, x, y, &s.ids);
            let moved: Vec<NodeId> = free.into_iter().filter(|id| doc.parent(*id) != Some(target)).collect();
            if !moved.is_empty() {
                // Current world rects: origin + the offset applied during the drag.
                let world: Vec<(NodeId, Rect)> = moved
                    .iter()
                    .filter_map(|id| {
                        let n = doc.node(*id)?;
                        let parent = doc.parent(*id)?;
                        let (ox, oy) = origin_of(doc, parent);
                        let r = doc.measured.get(id)?;
                        let left = n.style.left.as_ref().and_then(Len::px)?;
                        let top = n.style.top.as_ref().and_then(Len::px)?;
                        Some((*id, Rect { x: ox + left, y: oy + top, w: r.w, h: r.h }))
                    })
                    .collect();
                let index = if parent_is_flex(doc, target) {
                    let row = matches!(target, Parent::Node(p) if doc.node(p).unwrap().style.is_row());
                    Some(
                        doc.children(target)
                            .filter(|c| {
                                doc.measured
                                    .get(c)
                                    .is_some_and(|r| if row { r.x + r.w / 2.0 < x } else { r.y + r.h / 2.0 < y })
                            })
                            .count(),
                    )
                } else {
                    None
                };
                for (i, id) in moved.iter().enumerate() {
                    let o = reparent_ops(&open.editor.doc, &[*id], target, index.map(|n| n + i), &world);
                    ops.extend(open.editor.transact(o, None).map_err(|e| e.to_string())?);
                }
            }
        }
    }
    open.editor.commit();
    Ok(ops)
}

pub fn resize(open: &mut OpenDoc, target: Rect, snap: bool, zoom: f64) -> Result<(Value, Vec<Op>), String> {
    prune_session(open);
    let Some(s) = &open.session else { return Err("no resize in progress".into()) };
    let (target, guides) =
        if snap { s.snapper.snap_resize(&s.union, &target, 4.0 / zoom.max(0.01)) } else { (target, Vec::new()) };
    let sx = if s.union.w > 0.0 { target.w / s.union.w } else { 1.0 };
    let sy = if s.union.h > 0.0 { target.h / s.union.h } else { 1.0 };
    let doc = &open.editor.doc;
    let mut ops = Vec::new();
    for (id, r) in &s.origins {
        let n = doc.node(*id).unwrap();
        let nr = Rect {
            x: target.x + (r.x - s.union.x) * sx,
            y: target.y + (r.y - s.union.y) * sy,
            w: (r.w * sx).max(1.0),
            h: (r.h * sy).max(1.0),
        };
        let size = |v: f64| json!(Size::Fixed(Len::Px(round2(v))));
        if (nr.w - r.w).abs() > 1e-6 || n.style.width.is_none() && sx != 1.0 {
            ops.push(Op::Set { id: *id, prop: "style.width".into(), value: size(nr.w) });
        }
        if (nr.h - r.h).abs() > 1e-6 || n.style.height.is_none() && sy != 1.0 {
            ops.push(Op::Set { id: *id, prop: "style.height".into(), value: size(nr.h) });
        }
        if is_free(doc, *id) {
            let parent = doc.parent(*id).unwrap();
            ops.extend(
                place_ops(doc, *id, parent, nr.x, nr.y)
                    .into_iter()
                    .filter(|op| !matches!(op, Op::Set { prop, .. } if prop == "style.position")),
            );
        }
    }
    let ops = open.editor.transact(ops, None).map_err(|e| e.to_string())?;
    Ok((json!({"guides": guides}), ops))
}

pub fn nudge(open: &mut OpenDoc, ids: &[NodeId], dx: f64, dy: f64) -> Vec<Op> {
    let doc = &open.editor.doc;
    let mut ops = Vec::new();
    for id in roots(doc, ids) {
        if !is_free(doc, id) {
            continue;
        }
        let n = doc.node(id).unwrap();
        let parent = doc.parent(id).unwrap();
        let (ox, oy) = origin_of(doc, parent);
        let m = doc.measured.get(&id).copied().unwrap_or_default();
        let left = n.style.left.as_ref().and_then(Len::px).unwrap_or(m.x - ox);
        let top = n.style.top.as_ref().and_then(Len::px).unwrap_or(m.y - oy);
        if dx != 0.0 {
            ops.push(Op::Set { id, prop: "style.left".into(), value: px(left + dx) });
        }
        if dy != 0.0 {
            ops.push(Op::Set { id, prop: "style.top".into(), value: px(top + dy) });
        }
    }
    ops
}

/// Wraps sibling nodes in a new frame sized to their bounds. Returns the frame id.
pub fn wrap(open: &mut OpenDoc, ids: &[NodeId]) -> Result<(NodeId, Vec<Op>), String> {
    let doc = &mut open.editor.doc;
    let ids = roots(doc, ids);
    let parent = doc.parent(*ids.first().ok_or("nothing selected")?).ok_or("node not found")?;
    if ids.iter().any(|id| doc.parent(*id) != Some(parent)) {
        return Err("only siblings can be wrapped together".into());
    }
    let rects: Vec<(NodeId, Rect)> = ids.iter().filter_map(|id| Some((*id, *doc.measured.get(id)?))).collect();
    let u = union(rects.iter().map(|(_, r)| *r)).ok_or("nothing measured")?;
    let mut frame = Node::new(doc.alloc_node(), NodeKind::Frame);
    frame.style.width = Some(Size::Fixed(Len::Px(round2(u.w))));
    frame.style.height = Some(Size::Fixed(Len::Px(round2(u.h))));
    let fid = frame.id;
    let order: Vec<NodeId> = doc.children(parent).filter(|c| ids.contains(c)).collect();
    let first = doc.children(parent).position(|c| ids.contains(&c)).unwrap_or(0);
    let flex = parent_is_flex(doc, parent);
    if !flex {
        let (ox, oy) = origin_of(doc, parent);
        frame.style.position = Some("absolute".into());
        frame.style.left = Some(Len::Px(round2(u.x - ox)));
        frame.style.top = Some(Len::Px(round2(u.y - oy)));
    }
    let key = doc.key_at(parent, Some(first), None);
    doc.measured.insert(fid, u);
    let mut ops = vec![Op::Insert { node: frame, parent, key }];
    let keys = spark_core::fractional::keys_between(None, None, order.len());
    for (id, key) in order.iter().zip(keys) {
        ops.push(Op::Move { id: *id, parent: Parent::Node(fid), key });
        let r = rects.iter().find(|(w, _)| w == id).map(|(_, r)| *r).unwrap_or(u);
        ops.push(Op::Set { id: *id, prop: "style.position".into(), value: json!("absolute") });
        ops.push(Op::Set { id: *id, prop: "style.left".into(), value: px(r.x - u.x) });
        ops.push(Op::Set { id: *id, prop: "style.top".into(), value: px(r.y - u.y) });
    }
    Ok((fid, ops))
}

/// Turns a freeform frame into a flex container, ordering children by position.
pub fn add_flex(doc: &Document, id: NodeId) -> Vec<Op> {
    let Some(n) = doc.node(id).filter(|n| n.kind == NodeKind::Frame) else { return vec![] };
    if n.style.is_flex() {
        return vec![];
    }
    let kids: Vec<(NodeId, Rect)> =
        doc.children(Parent::Node(id)).filter_map(|c| Some((c, *doc.measured.get(&c)?))).collect();
    let frame = doc.measured.get(&id).copied().unwrap_or_default();
    let spread_x = kids.iter().map(|(_, r)| r.right()).fold(f64::MIN, f64::max)
        - kids.iter().map(|(_, r)| r.x).fold(f64::MAX, f64::min);
    let spread_y = kids.iter().map(|(_, r)| r.bottom()).fold(f64::MIN, f64::max)
        - kids.iter().map(|(_, r)| r.y).fold(f64::MAX, f64::min);
    let sum_w: f64 = kids.iter().map(|(_, r)| r.w).sum();
    let sum_h: f64 = kids.iter().map(|(_, r)| r.h).sum();
    // Row when the children are laid out side by side more than stacked.
    let row = kids.len() > 1
        && (spread_x - sum_w) > (spread_y - sum_h)
        && spread_x / frame.w.max(1.0) > spread_y / frame.h.max(1.0);
    let mut sorted = kids.clone();
    sorted.sort_by(|a, b| if row { a.1.x.total_cmp(&b.1.x) } else { a.1.y.total_cmp(&b.1.y) });
    let gaps: Vec<f64> = sorted
        .windows(2)
        .map(|w| if row { w[1].1.x - w[0].1.right() } else { w[1].1.y - w[0].1.bottom() })
        .filter(|g| *g >= 0.0)
        .collect();
    let gap = if gaps.is_empty() { 0.0 } else { (gaps.iter().sum::<f64>() / gaps.len() as f64).round() };
    let pad = |v: f64| round2(v.max(0.0));
    let (mut ops, keys) = (Vec::new(), spark_core::fractional::keys_between(None, None, sorted.len()));
    ops.push(Op::Set { id, prop: "style.display".into(), value: json!("flex") });
    ops.push(Op::Set { id, prop: "style.flexDirection".into(), value: json!(if row { "row" } else { "column" }) });
    ops.push(Op::Set { id, prop: "style.gap".into(), value: px(gap) });
    if !kids.is_empty() {
        let minx = kids.iter().map(|(_, r)| r.x).fold(f64::MAX, f64::min);
        let miny = kids.iter().map(|(_, r)| r.y).fold(f64::MAX, f64::min);
        let p = [pad(miny - frame.y), pad(minx - frame.x), pad(miny - frame.y), pad(minx - frame.x)];
        ops.push(Op::Set { id, prop: "style.padding".into(), value: json!(p.map(|v| format!("{v}px"))) });
    }
    for ((cid, _), key) in sorted.iter().zip(keys) {
        ops.push(Op::Move { id: *cid, parent: Parent::Node(id), key });
        let c = doc.node(*cid).unwrap();
        for f in ["position", "left", "top", "right", "bottom"] {
            if !c.style.get_field(f).is_null() {
                ops.push(Op::Set { id: *cid, prop: format!("style.{f}"), value: Value::Null });
            }
        }
    }
    ops
}

/// Turns a flex container back into a freeform frame, pinning children where they are now.
pub fn remove_flex(doc: &Document, id: NodeId) -> Vec<Op> {
    let Some(n) = doc.node(id) else { return vec![] };
    let mut ops: Vec<Op> = ["display", "flexDirection", "flexWrap", "alignItems", "justifyContent", "gap"]
        .iter()
        .filter(|f| !n.style.get_field(f).is_null())
        .map(|f| Op::Set { id, prop: format!("style.{f}"), value: Value::Null })
        .collect();
    let (ox, oy) = origin_of(doc, Parent::Node(id));
    for c in doc.children(Parent::Node(id)) {
        let Some(r) = doc.measured.get(&c) else { continue };
        ops.push(Op::Set { id: c, prop: "style.position".into(), value: json!("absolute") });
        ops.push(Op::Set { id: c, prop: "style.left".into(), value: px(r.x - ox) });
        ops.push(Op::Set { id: c, prop: "style.top".into(), value: px(r.y - oy) });
        // Fill sizing only makes sense in flow; pin the measured size instead.
        let cn = doc.node(c).unwrap();
        if matches!(cn.style.width, Some(Size::Fill)) {
            ops.push(Op::Set { id: c, prop: "style.width".into(), value: px(r.w) });
        }
        if matches!(cn.style.height, Some(Size::Fill)) {
            ops.push(Op::Set { id: c, prop: "style.height".into(), value: px(r.h) });
        }
    }
    ops
}

pub fn zorder(doc: &Document, ids: &[NodeId], dir: &str) -> Vec<Op> {
    let mut ops = Vec::new();
    for id in roots(doc, ids) {
        let Some(parent) = doc.parent(id) else { continue };
        let sibs: Vec<NodeId> = doc.children(parent).collect();
        let i = sibs.iter().position(|s| *s == id).unwrap();
        let index = match dir {
            "bringForward" if i + 1 < sibs.len() => i + 1,
            "sendBackward" if i > 0 => i - 1,
            "bringToFront" if i + 1 < sibs.len() => sibs.len() - 1,
            "sendToBack" if i > 0 => 0,
            _ => continue,
        };
        ops.push(Op::Move { id, parent, key: doc.key_at(parent, Some(index), Some(id)) });
    }
    ops
}

/// Where pasted or imported roots go: the parent, and an optional world point for their top-left.
pub struct Placement {
    pub parent: Parent,
    pub at: Option<(f64, f64)>,
}

/// Positions freshly inserted roots: absolutely at the paste point inside freeform parents (or in
/// free space on the page), and in flow inside flex parents. Applies to the ops before they run.
pub fn position_roots(doc: &Document, ops: &mut [Op], roots: &[NodeId], place: &Placement, sizes: &[(NodeId, Rect)]) {
    let flex = parent_is_flex(doc, place.parent);
    let at = place.at.or_else(|| match place.parent {
        Parent::Page(page) => Some(free_spot(doc, page)),
        Parent::Node(_) => None,
    });
    let u = union(sizes.iter().map(|(_, r)| *r));
    let mut y_cursor = 0.0;
    for op in ops.iter_mut() {
        let Op::Insert { node, .. } = op else { continue };
        if !roots.contains(&node.id) {
            continue;
        }
        let s = &mut node.style;
        if flex {
            s.position = None;
            s.left = None;
            s.top = None;
            s.right = None;
            s.bottom = None;
            continue;
        }
        let Some((x, y)) = at else { continue };
        let (ox, oy) = origin_of(doc, place.parent);
        // Keep the roots' layout relative to each other when we know their old rects.
        let (dx, dy) = match (sizes.iter().find(|(id, _)| *id == node.id), u) {
            (Some((_, r)), Some(u)) => (r.x - u.x, r.y - u.y),
            _ => {
                let off = (0.0, y_cursor);
                y_cursor += 40.0;
                off
            }
        };
        s.position = Some("absolute".into());
        s.left = Some(Len::Px(round2(x + dx - ox)));
        s.top = Some(Len::Px(round2(y + dy - oy)));
        s.right = None;
        s.bottom = None;
    }
}

/// New root ids, the insert ops, and the roots' original rects (for keeping their arrangement).
pub type Pasted = (Vec<NodeId>, Vec<Op>, Vec<(NodeId, Rect)>);

/// Recreates nodes from Spark's clipboard format (node trees) with fresh ids.
pub fn spark_paste_ops(doc: &mut Document, roots: &[Value], parent: Parent) -> Result<Pasted, String> {
    fn walk(
        doc: &mut Document,
        tree: &Value,
        parent: Parent,
        key: String,
        ops: &mut Vec<Op>,
    ) -> Result<NodeId, String> {
        let mut node: Node =
            serde_json::from_value(tree["node"].clone()).map_err(|e| format!("bad clipboard data: {e}"))?;
        node.id = doc.alloc_node();
        node.deleted = false;
        let id = node.id;
        if node.kind == NodeKind::Vector && node.tag.is_none() {
            return Err("bad clipboard data: vector without tag".into());
        }
        ops.push(Op::Insert { node, parent, key });
        let kids = tree["children"].as_array().cloned().unwrap_or_default();
        let keys = spark_core::fractional::keys_between(None, None, kids.len());
        for (k, key) in kids.iter().zip(keys) {
            walk(doc, k, Parent::Node(id), key, ops)?;
        }
        Ok(id)
    }
    let mut ops = Vec::new();
    let mut ids = Vec::new();
    let mut rects = Vec::new();
    let start = doc.child_keys(parent).last().map(|(k, _)| k.clone());
    let keys = spark_core::fractional::keys_between(start.as_deref(), None, roots.len());
    for (tree, key) in roots.iter().zip(keys) {
        let id = walk(doc, tree, parent, key, &mut ops)?;
        if let Ok(r) = serde_json::from_value::<Rect>(tree["rect"].clone()) {
            rects.push((id, r));
        }
        ids.push(id);
    }
    Ok((ids, ops, rects))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    fn snapper(rects: &[Rect]) -> Snapper {
        Snapper {
            tree: RTree::bulk_load(
                rects.iter().enumerate().map(|(i, q)| entry(NodeId::new(i as u64 + 1, 0), q)).collect(),
            ),
        }
    }

    #[test]
    fn snaps_edges_within_threshold() {
        let s = snapper(&[r(0.0, 0.0, 100.0, 100.0)]);
        let (dx, dy, guides) = s.snap_move(&r(103.0, 300.0, 50.0, 50.0), 4.0);
        assert_eq!((dx, dy), (-3.0, 0.0), "left edge snaps to the neighbor's right edge");
        assert!(guides.iter().any(|g| g.x1 == 100.0 && g.x2 == 100.0 && !g.dashed));
        let (dx, _, _) = s.snap_move(&r(110.0, 300.0, 50.0, 50.0), 4.0);
        assert_eq!(dx, 0.0, "too far to snap");
    }

    #[test]
    fn snaps_to_equal_spacing() {
        let s = snapper(&[r(0.0, 0.0, 100.0, 100.0), r(300.0, 0.0, 100.0, 100.0)]);
        let (dx, _, guides) = s.snap_move(&r(152.0, 20.0, 100.0, 60.0), 4.0);
        assert_eq!(dx, -2.0);
        assert_eq!(guides.iter().filter(|g| g.dashed).count(), 2);
    }

    #[test]
    fn resize_snaps_only_moving_edges() {
        let s = snapper(&[r(0.0, 0.0, 100.0, 100.0)]);
        let from = r(200.0, 0.0, 50.0, 50.0);
        let (out, _) = s.snap_resize(&from, &r(200.0, 0.0, 50.0, 98.0), 4.0);
        assert_eq!(out.h, 100.0);
        assert_eq!(out.x, 200.0);
    }
}
