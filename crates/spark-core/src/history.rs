//! Undo and redo: each transaction stores its forward ops, their inverses, and the selection and
//! page around it. Drags group with begin/commit, rapid edits with the same merge key coalesce
//! within 250 ms, and history keeps the last 250 transactions.

use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use crate::{
    doc::{Document, Op, OpError, Parent},
    id::{NodeId, PageId},
    style::Len,
};

/// An artboard's page and its px left/top (unset counts as 0); None for other nodes or non-px values.
fn placed(doc: &Document, id: NodeId) -> Option<(Parent, f64, f64)> {
    let parent @ Parent::Page(_) = doc.parent(id)? else { return None };
    let px = |l: &Option<Len>| l.as_ref().map_or(Some(0.0), Len::px);
    let n = doc.node(id)?;
    Some((parent, px(&n.style.left)?, px(&n.style.top)?))
}

pub const MERGE_WINDOW: Duration = Duration::from_millis(250);
pub const MAX_ENTRIES: usize = 250;

#[derive(Debug, Clone)]
pub struct Transaction {
    pub ops: Vec<Op>,
    /// Inverse ops in the order they must be applied to undo.
    pub inverse: Vec<Op>,
    pub selection_before: Vec<NodeId>,
    pub selection_after: Vec<NodeId>,
    pub page: Option<PageId>,
    merge: Option<String>,
    at: Instant,
}

/// A document plus its undo history and the user's selection and viewed page.
#[derive(Debug)]
pub struct Editor {
    pub doc: Document,
    pub selection: Vec<NodeId>,
    pub page: Option<PageId>,
    undo: VecDeque<Transaction>,
    redo: Vec<Transaction>,
    group: Option<Transaction>,
}

impl Editor {
    pub fn new(doc: Document) -> Editor {
        let page = doc.first_page();
        Editor { doc, selection: Vec::new(), page, undo: VecDeque::new(), redo: Vec::new(), group: None }
    }

    fn empty_tx(&self, merge: Option<String>) -> Transaction {
        Transaction {
            ops: Vec::new(),
            inverse: Vec::new(),
            selection_before: self.selection.clone(),
            selection_after: self.selection.clone(),
            page: self.page,
            merge,
            at: Instant::now(),
        }
    }

    /// Applies `ops` atomically: if any op fails, the ones already applied are rolled back.
    pub fn apply_all(doc: &mut Document, ops: Vec<Op>) -> Result<(Vec<Op>, Vec<Op>), OpError> {
        // An artboard that stays on its page while its left/top change moved on the canvas: carry what
        // was measured inside it along, so reads are right before the UI re-measures (it skips LOD
        // shells). Reparenting rewrites left/top too, but to keep the world position: no shift then.
        let mut placed_before: Vec<(NodeId, (Parent, f64, f64))> = Vec::new();
        // Nodes landing on a page from another parent: their left/top now say where they are.
        let mut arriving: Vec<(NodeId, Option<Parent>)> = Vec::new();
        for op in &ops {
            if let Op::Move { id, parent: Parent::Page(_), .. } = op
                && !arriving.iter().any(|(a, _)| a == id)
            {
                arriving.push((*id, doc.parent(*id)));
            }
            if let Op::Set { id, prop, .. } = op
                && (prop == "style.left" || prop == "style.top")
                && !placed_before.iter().any(|(p, _)| p == id)
                && let Some(at) = placed(doc, *id)
            {
                placed_before.push((*id, at));
            }
        }
        let mut done = Vec::with_capacity(ops.len());
        let mut inverse = Vec::with_capacity(ops.len());
        for op in ops {
            match doc.apply(op.clone()) {
                Ok(inv) => {
                    done.push(op);
                    inverse.extend(inv);
                }
                Err(e) => {
                    for inv in inverse.into_iter().rev() {
                        doc.apply(inv).expect("rollback");
                    }
                    return Err(e);
                }
            }
        }
        inverse.reverse();
        for (id, (parent, x, y)) in placed_before {
            if let Some((p, nx, ny)) = placed(doc, id)
                && p == parent
                && (nx != x || ny != y)
            {
                doc.shift_measured(id, nx - x, ny - y);
            }
        }
        // Undoing a drag that dropped an artboard into a frame brings it back with its old left/top:
        // line the measured rects up with them (a rotated artboard measures its rotated box, so skip).
        for (id, before) in arriving {
            if let Some((p, x, y)) = placed(doc, id)
                && Some(p) != before
                && doc.node(id).is_some_and(|n| n.style.rotate.unwrap_or(0.0) == 0.0)
                && let Some(m) = doc.measured.get(&id).copied()
                && (m.x != x || m.y != y)
            {
                doc.shift_measured(id, x - m.x, y - m.y);
            }
        }
        Ok((done, inverse))
    }

    /// Applies ops as one undoable step. A `merge` key folds the step into the previous one when it
    /// has the same key and came within 250 ms (typing in a field, scrubbing a value).
    pub fn transact(&mut self, ops: Vec<Op>, merge: Option<&str>) -> Result<Vec<Op>, OpError> {
        let (done, inverse) = Self::apply_all(&mut self.doc, ops)?;
        if done.is_empty() {
            return Ok(done);
        }
        self.redo.clear();
        let selection = self.selection.clone();
        if let Some(group) = &mut self.group {
            group.ops.extend(done.iter().cloned());
            group.inverse.splice(0..0, inverse);
            group.selection_after = selection;
            return Ok(done);
        }
        let now = Instant::now();
        if let (Some(key), Some(last)) = (merge, self.undo.back_mut())
            && last.merge.as_deref() == Some(key)
            && now.duration_since(last.at) < MERGE_WINDOW
        {
            last.ops.extend(done.iter().cloned());
            last.inverse.splice(0..0, inverse);
            last.selection_after = selection;
            last.at = now;
            return Ok(done);
        }
        let mut tx = self.empty_tx(merge.map(str::to_string));
        tx.ops = done.clone();
        tx.inverse = inverse;
        self.push(tx);
        Ok(done)
    }

    /// Applies ops without recording history (bookkeeping, or replaying remote changes).
    pub fn apply_unrecorded(&mut self, ops: Vec<Op>) -> Result<Vec<Op>, OpError> {
        Self::apply_all(&mut self.doc, ops).map(|(done, _)| done)
    }

    fn push(&mut self, tx: Transaction) {
        self.undo.push_back(tx);
        while self.undo.len() > MAX_ENTRIES {
            self.undo.pop_front();
        }
    }

    /// Starts a group: every transaction until `commit` becomes one undo step.
    pub fn begin(&mut self) {
        if self.group.is_none() {
            self.group = Some(self.empty_tx(None));
        }
    }

    pub fn commit(&mut self) {
        if let Some(tx) = self.group.take()
            && !tx.ops.is_empty()
        {
            self.push(tx);
        }
    }

    /// Ends a group by reverting everything in it. Returns the ops applied to revert.
    pub fn cancel(&mut self) -> Vec<Op> {
        let Some(tx) = self.group.take() else { return Vec::new() };
        self.selection = tx.selection_before;
        Self::apply_all(&mut self.doc, tx.inverse).map(|(done, _)| done).unwrap_or_default()
    }

    /// Sets an open group aside (a UI gesture in progress) so other changes form their own undo
    /// step; `resume_group` gives it back.
    pub fn suspend_group(&mut self) -> Option<Transaction> {
        self.group.take()
    }

    pub fn resume_group(&mut self, group: Option<Transaction>) {
        if group.is_some() {
            self.group = group;
        }
    }

    pub fn in_group(&self) -> bool {
        self.group.is_some()
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.group.as_ref().is_some_and(|g| !g.ops.is_empty())
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Undoes the last step. Returns the ops applied, or None if there was nothing to undo.
    pub fn undo(&mut self) -> Option<Vec<Op>> {
        self.commit();
        let tx = self.undo.pop_back()?;
        let applied = match Self::apply_all(&mut self.doc, tx.inverse.clone()) {
            Ok((done, _)) => done,
            Err(_) => return Some(Vec::new()),
        };
        self.selection = tx.selection_before.clone();
        self.page = tx.page.or(self.page);
        self.redo.push(tx);
        Some(applied)
    }

    pub fn redo(&mut self) -> Option<Vec<Op>> {
        let tx = self.redo.pop()?;
        let applied = match Self::apply_all(&mut self.doc, tx.ops.clone()) {
            Ok((done, _)) => done,
            Err(_) => return Some(Vec::new()),
        };
        self.selection = tx.selection_after.clone();
        self.page = tx.page.or(self.page);
        self.undo.push_back(tx);
        Some(applied)
    }

    pub fn history_len(&self) -> usize {
        self.undo.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        doc::Parent,
        node::{Node, NodeKind},
    };
    use serde_json::json;

    fn editor() -> (Editor, crate::NodeId) {
        let mut doc = Document::new("H");
        let page = doc.pages[0].id;
        let id = doc.alloc_node();
        let mut ed = Editor::new(doc);
        let key = ed.doc.key_at(Parent::Page(page), None, None);
        ed.transact(vec![Op::Insert { node: Node::new(id, NodeKind::Frame), parent: Parent::Page(page), key }], None)
            .unwrap();
        (ed, id)
    }

    fn width(ed: &Editor, id: crate::NodeId) -> serde_json::Value {
        ed.doc.node(id).unwrap().style.get_field("width")
    }

    fn set_width(ed: &mut Editor, id: crate::NodeId, w: u32, merge: Option<&str>) {
        ed.transact(vec![Op::Set { id, prop: "style.width".into(), value: json!(format!("{w}px")) }], merge).unwrap();
    }

    #[test]
    fn moving_an_artboard_moves_what_was_measured_inside_it() {
        let (mut ed, board) = editor();
        let child = ed.doc.alloc_node();
        let key = ed.doc.key_at(Parent::Node(board), None, None);
        ed.transact(
            vec![Op::Insert { node: Node::new(child, NodeKind::Frame), parent: Parent::Node(board), key }],
            None,
        )
        .unwrap();
        ed.doc.set_measured(board, crate::doc::Rect { x: 0.0, y: 0.0, w: 400.0, h: 300.0 });
        ed.doc.set_measured(child, crate::doc::Rect { x: 10.0, y: 20.0, w: 100.0, h: 50.0 });
        ed.transact(vec![Op::Set { id: board, prop: "style.left".into(), value: json!("300px") }], None).unwrap();
        assert_eq!((ed.doc.measured[&board].x, ed.doc.measured[&child].x), (300.0, 310.0));
        ed.undo().unwrap();
        assert_eq!((ed.doc.measured[&board].x, ed.doc.measured[&child].x), (0.0, 10.0));
    }

    #[test]
    fn promoting_a_layer_to_an_artboard_keeps_where_it_was() {
        let (mut ed, frame) = editor();
        let page = ed.doc.pages[0].id;
        let child = ed.doc.alloc_node();
        let key = ed.doc.key_at(Parent::Node(frame), None, None);
        ed.transact(
            vec![Op::Insert { node: Node::new(child, NodeKind::Frame), parent: Parent::Node(frame), key }],
            None,
        )
        .unwrap();
        ed.transact(vec![Op::Set { id: child, prop: "style.left".into(), value: json!("20px") }], None).unwrap();
        ed.doc.set_measured(frame, crate::doc::Rect { x: 100.0, y: 0.0, w: 400.0, h: 300.0 });
        ed.doc.set_measured(child, crate::doc::Rect { x: 120.0, y: 0.0, w: 50.0, h: 50.0 });
        // What reparenting emits: move to the page, then left/top in page coordinates.
        let key = ed.doc.key_at(Parent::Page(page), None, Some(child));
        ed.transact(
            vec![
                Op::Move { id: child, parent: Parent::Page(page), key },
                Op::Set { id: child, prop: "style.left".into(), value: json!("120px") },
                Op::Set { id: child, prop: "style.top".into(), value: json!("0px") },
            ],
            None,
        )
        .unwrap();
        assert_eq!(ed.doc.measured[&child].x, 120.0);
        ed.undo().unwrap();
        assert_eq!(ed.doc.measured[&child].x, 120.0);
    }

    #[test]
    fn undoing_a_drag_into_a_frame_puts_the_artboard_back_where_it_was() {
        let (mut ed, board) = editor();
        let page = ed.doc.pages[0].id;
        let frame = ed.doc.alloc_node();
        let key = ed.doc.key_at(Parent::Page(page), None, None);
        ed.transact(
            vec![Op::Insert { node: Node::new(frame, NodeKind::Frame), parent: Parent::Page(page), key }],
            None,
        )
        .unwrap();
        ed.transact(vec![Op::Set { id: frame, prop: "style.left".into(), value: json!("1000px") }], None).unwrap();
        ed.doc.set_measured(board, crate::doc::Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 });
        ed.doc.set_measured(frame, crate::doc::Rect { x: 1000.0, y: 0.0, w: 400.0, h: 300.0 });
        // A drag: move to x=600, then drop into the frame (reparent keeps the world position).
        ed.begin();
        ed.transact(vec![Op::Set { id: board, prop: "style.left".into(), value: json!("600px") }], None).unwrap();
        let key = ed.doc.key_at(Parent::Node(frame), None, Some(board));
        ed.transact(
            vec![
                Op::Move { id: board, parent: Parent::Node(frame), key },
                Op::Set { id: board, prop: "style.left".into(), value: json!("-400px") },
            ],
            None,
        )
        .unwrap();
        ed.commit();
        assert_eq!(ed.doc.measured[&board].x, 600.0);
        ed.undo().unwrap();
        assert_eq!(ed.doc.parent(board), Some(Parent::Page(page)));
        assert_eq!(ed.doc.measured[&board].x, 0.0);
    }

    #[test]
    fn undo_redo_round_trip_with_selection() {
        let (mut ed, id) = editor();
        ed.selection = vec![id];
        set_width(&mut ed, id, 10, None);
        ed.selection = vec![];
        assert_eq!(width(&ed, id), json!("10px"));
        ed.undo().unwrap();
        assert_eq!(width(&ed, id), json!(null));
        assert_eq!(ed.selection, vec![id]);
        ed.redo().unwrap();
        assert_eq!(width(&ed, id), json!("10px"));
        ed.undo().unwrap();
        ed.undo().unwrap();
        assert!(ed.doc.node(id).is_none(), "insert undone");
        ed.redo().unwrap();
        assert!(ed.doc.node(id).is_some(), "insert redone");
    }

    #[test]
    fn same_key_edits_merge_within_window() {
        let (mut ed, id) = editor();
        let before = ed.history_len();
        set_width(&mut ed, id, 1, Some("w"));
        set_width(&mut ed, id, 2, Some("w"));
        set_width(&mut ed, id, 3, Some("w"));
        assert_eq!(ed.history_len(), before + 1);
        std::thread::sleep(MERGE_WINDOW + Duration::from_millis(20));
        set_width(&mut ed, id, 4, Some("w"));
        assert_eq!(ed.history_len(), before + 2);
        ed.undo();
        assert_eq!(width(&ed, id), json!("3px"));
        ed.undo();
        assert_eq!(width(&ed, id), json!(null));
        set_width(&mut ed, id, 5, None);
        set_width(&mut ed, id, 6, None);
        assert_eq!(ed.history_len(), before + 2, "unkeyed edits never merge");
    }

    #[test]
    fn groups_make_one_step_and_cancel_reverts() {
        let (mut ed, id) = editor();
        let before = ed.history_len();
        ed.begin();
        for w in 1..=20 {
            set_width(&mut ed, id, w, None);
        }
        ed.commit();
        assert_eq!(ed.history_len(), before + 1);
        ed.undo();
        assert_eq!(width(&ed, id), json!(null));
        ed.begin();
        set_width(&mut ed, id, 99, None);
        ed.cancel();
        assert_eq!(width(&ed, id), json!(null));
        assert!(!ed.can_redo() || ed.history_len() == before);
    }

    #[test]
    fn failed_transaction_rolls_back() {
        let (mut ed, id) = editor();
        let ops = vec![
            Op::Set { id, prop: "name".into(), value: json!("Changed") },
            Op::Set { id, prop: "style.width".into(), value: json!("nonsense") },
        ];
        assert!(ed.transact(ops, None).is_err());
        assert_eq!(ed.doc.node(id).unwrap().name, "Frame");
    }

    #[test]
    fn history_is_capped() {
        let (mut ed, id) = editor();
        for w in 0..(MAX_ENTRIES as u32 + 20) {
            set_width(&mut ed, id, w, None);
        }
        assert_eq!(ed.history_len(), MAX_ENTRIES);
    }
}
