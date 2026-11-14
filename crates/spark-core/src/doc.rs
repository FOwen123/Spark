//! The document and its closed operation set. Every change goes through [`Document::apply`],
//! which validates the op, mutates the document and returns the inverse op.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    str::FromStr,
    sync::Arc,
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    fractional::{is_valid_key, key_between},
    id::{IdError, NodeId, PageId, string_serde},
    node::{LayoutGuide, Node, NodeKind, PAGE_ICONS, Page},
    tokens::{Token, validate_name},
};

/// Where a node sits: directly on a page (an artboard) or inside another node.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Parent {
    Page(PageId),
    Node(NodeId),
}

impl fmt::Display for Parent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Parent::Page(p) => p.fmt(f),
            Parent::Node(n) => n.fmt(f),
        }
    }
}

impl FromStr for Parent {
    type Err = IdError;
    fn from_str(s: &str) -> Result<Parent, IdError> {
        if s.starts_with("p-") { s.parse().map(Parent::Page) } else { s.parse().map(Parent::Node) }
    }
}
string_serde!(Parent);

/// An embedded, content-addressed binary (images).
#[derive(Clone, PartialEq, Debug)]
pub struct Asset {
    pub mime: String,
    pub bytes: Arc<Vec<u8>>,
}

/// A world-space rectangle measured by the UI.
#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.right() && o.x < self.right() && self.y < o.bottom() && o.y < self.bottom()
    }
}

// Ops are short-lived values moved straight into `apply`; boxing `Insert` would only add noise.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Op {
    /// Add a node (or restore a deleted one with the same id).
    Insert {
        node: Node,
        parent: Parent,
        key: String,
    },
    /// Tombstone a node; its subtree goes with it.
    Delete {
        id: NodeId,
    },
    Move {
        id: NodeId,
        parent: Parent,
        key: String,
    },
    /// Set one property: `name`, `hidden`, `locked`, `text`, `tag`, `attrs`, `exports`, `layoutGuides` (frames), `image`,
    /// `style` or `style.<field>`. `null` clears.
    Set {
        id: NodeId,
        prop: String,
        value: Value,
    },
    InsertPage {
        page: Page,
        index: usize,
    },
    DeletePage {
        id: PageId,
    },
    /// Set a page property: `name`, `icon` or `canvas`.
    SetPage {
        id: PageId,
        prop: String,
        value: Value,
    },
    MovePage {
        id: PageId,
        index: usize,
    },
    /// Create, change (`Some`) or remove (`None`) a token.
    SetToken {
        name: String,
        token: Option<Token>,
    },
    /// Add an embedded asset. Assets are immutable, so this op has no inverse.
    AddAsset {
        hash: String,
        mime: String,
        #[serde(skip)]
        bytes: Arc<Vec<u8>>,
    },
    /// Rename the file.
    SetFileName {
        name: String,
    },
    /// Replace the style guide.
    SetGuide {
        text: String,
    },
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum OpError {
    #[error("node {0} not found")]
    NoNode(NodeId),
    #[error("page {0} not found")]
    NoPage(PageId),
    #[error("node {0} already exists")]
    Exists(NodeId),
    #[error("page {0} already exists")]
    PageExists(PageId),
    #[error("{0}")]
    Invalid(String),
}

/// The style guide's size limit; it is sent to agents with `get_basic_info`.
pub const MAX_GUIDE_BYTES: usize = 100_000;

/// How deep frames may nest below an artboard. Renderers recurse per level, so this bounds their stack use.
pub const MAX_DEPTH: usize = 100;

fn invalid<T>(msg: impl Into<String>) -> Result<T, OpError> {
    Err(OpError::Invalid(msg.into()))
}

/// What changed since the last save.
#[derive(Default, Debug)]
pub struct Dirty {
    pub nodes: HashSet<NodeId>,
    pub pages: bool,
    pub tokens: HashSet<String>,
    pub assets: HashSet<String>,
    pub layout: HashSet<NodeId>,
    pub meta: bool,
}

impl Dirty {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
            && !self.pages
            && self.tokens.is_empty()
            && self.assets.is_empty()
            && self.layout.is_empty()
            && !self.meta
    }

    pub fn merge(&mut self, other: Dirty) {
        self.nodes.extend(other.nodes);
        self.pages |= other.pages;
        self.tokens.extend(other.tokens);
        self.assets.extend(other.assets);
        self.layout.extend(other.layout);
        self.meta |= other.meta;
    }
}

#[derive(Debug, Default)]
pub struct Document {
    pub id: String,
    pub name: String,
    pub pages: Vec<Page>,
    pub(crate) nodes: HashMap<NodeId, Node>,
    pub(crate) tree: HashMap<NodeId, (Parent, String)>,
    children: HashMap<Parent, Vec<(String, NodeId)>>,
    pub tokens: BTreeMap<String, Token>,
    /// The file's style guide: markdown rules and decisions that agents read before designing.
    pub guide: String,
    pub assets: BTreeMap<String, Asset>,
    /// Next node and page counters (file-wide, outside undo history).
    pub next_node: u64,
    pub next_page: u64,
    /// This client's site number; 0 for single-user editing.
    pub site: u32,
    /// Last measured world rects, reported by the UI. Bookkeeping only.
    pub measured: HashMap<NodeId, Rect>,
    pub dirty: Dirty,
}

impl Document {
    /// A new document with one empty page.
    pub fn new(name: &str) -> Document {
        let mut doc = Document {
            id: crate::id::new_file_id(),
            name: name.into(),
            next_node: 1,
            next_page: 1,
            ..Default::default()
        };
        let id = doc.alloc_page();
        doc.pages.push(Page { id, name: "Page 1".into(), icon: None, canvas: None, divider: false });
        doc.dirty.pages = true;
        doc.dirty.meta = true;
        doc
    }

    /// Builds a document from stored parts (used by persistence).
    pub fn from_parts(
        id: String,
        name: String,
        pages: Vec<Page>,
        nodes: HashMap<NodeId, Node>,
        tree: HashMap<NodeId, (Parent, String)>,
    ) -> Document {
        let mut doc = Document { id, name, pages, nodes, tree, next_node: 1, next_page: 1, ..Default::default() };
        for (id, (parent, key)) in &doc.tree {
            doc.children.entry(*parent).or_default().push((key.clone(), *id));
        }
        for list in doc.children.values_mut() {
            list.sort_unstable();
        }
        doc.next_node = doc.nodes.keys().filter(|n| n.site == doc.site).map(|n| n.counter + 1).max().unwrap_or(1);
        doc.next_page = doc.pages.iter().map(|p| p.id.0 + 1).max().unwrap_or(1);
        doc
    }

    pub fn alloc_node(&mut self) -> NodeId {
        let id = NodeId::new(self.next_node, self.site);
        self.next_node += 1;
        self.dirty.meta = true;
        id
    }

    pub fn alloc_page(&mut self) -> PageId {
        let id = PageId(self.next_page);
        self.next_page += 1;
        self.dirty.meta = true;
        id
    }

    /// Moves the measured rects of `id` and everything inside it by (dx, dy).
    pub fn shift_measured(&mut self, id: NodeId, dx: f64, dy: f64) {
        for n in std::iter::once(id).chain(self.descendants(Parent::Node(id))) {
            if let Some(mut r) = self.measured.get(&n).copied() {
                r.x += dx;
                r.y += dy;
                self.set_measured(n, r);
            }
        }
    }

    /// Records a measured rect from the UI (not an op; stays out of history).
    pub fn set_measured(&mut self, id: NodeId, rect: Rect) {
        if self.measured.insert(id, rect) != Some(rect) {
            self.dirty.layout.insert(id);
        }
    }

    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(&id).filter(|n| !n.deleted)
    }

    /// All stored nodes, including tombstones.
    pub fn all_nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values()
    }

    pub fn tree_entry(&self, id: NodeId) -> Option<&(Parent, String)> {
        self.tree.get(&id)
    }

    pub fn parent(&self, id: NodeId) -> Option<Parent> {
        self.tree.get(&id).map(|(p, _)| *p)
    }

    pub fn parent_node(&self, id: NodeId) -> Option<&Node> {
        match self.parent(id)? {
            Parent::Node(p) => self.node(p),
            Parent::Page(_) => None,
        }
    }

    /// Children in order (sort key, then id).
    pub fn children(&self, parent: Parent) -> impl DoubleEndedIterator<Item = NodeId> + ExactSizeIterator + '_ {
        self.children.get(&parent).map(|v| v.as_slice()).unwrap_or(&[]).iter().map(|(_, id)| *id)
    }

    pub fn child_keys(&self, parent: Parent) -> &[(String, NodeId)] {
        self.children.get(&parent).map(|v| v.as_slice()).unwrap_or(&[])
    }

    pub fn page(&self, id: PageId) -> Option<&Page> {
        self.pages.iter().find(|p| p.id == id)
    }

    /// The first non-divider page.
    pub fn first_page(&self) -> Option<PageId> {
        self.pages.iter().find(|p| !p.divider).map(|p| p.id)
    }

    /// The page a live node is on, or None if it is deleted or detached.
    pub fn page_of(&self, mut id: NodeId) -> Option<PageId> {
        for _ in 0..10_000 {
            self.node(id)?;
            match self.parent(id)? {
                Parent::Page(p) => return self.page(p).map(|_| p),
                Parent::Node(n) => id = n,
            }
        }
        None
    }

    pub fn is_live(&self, id: NodeId) -> bool {
        self.page_of(id).is_some()
    }

    /// The top-level frame (artboard) that contains `id`, possibly `id` itself.
    pub fn artboard_of(&self, mut id: NodeId) -> Option<NodeId> {
        loop {
            match self.parent(id)? {
                Parent::Page(_) => return Some(id),
                Parent::Node(n) => id = n,
            }
        }
    }

    /// Ancestors from the parent up to the artboard.
    pub fn ancestors(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut cur = id;
        while let Some(Parent::Node(p)) = self.parent(cur) {
            out.push(p);
            cur = p;
        }
        out
    }

    /// How deep a child of `parent` sits (artboards are 0).
    pub fn depth_below(&self, parent: Parent) -> usize {
        match parent {
            Parent::Page(_) => 0,
            Parent::Node(p) => self.ancestors(p).len() + 1,
        }
    }

    /// A node nested deeper than [`MAX_DEPTH`] or caught in a parent loop; only a damaged file has one.
    pub fn too_deep(&self) -> Option<NodeId> {
        self.tree.keys().copied().find(|id| {
            let mut cur = *id;
            (0..=MAX_DEPTH).all(|_| match self.parent(cur) {
                Some(Parent::Node(p)) => {
                    cur = p;
                    true
                }
                _ => false,
            })
        })
    }

    /// Depth-first, pre-order list of live node ids under `parent`.
    pub fn descendants(&self, parent: Parent) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack: Vec<NodeId> = self.children(parent).rev().collect();
        while let Some(id) = stack.pop() {
            if self.node(id).is_some() {
                out.push(id);
                stack.extend(self.children(Parent::Node(id)).rev());
            }
        }
        out
    }

    /// Number of live nodes across all pages.
    pub fn node_count(&self) -> usize {
        self.pages.iter().map(|p| self.descendants(Parent::Page(p.id)).len()).sum()
    }

    /// A key that places a new child at `index` (or the end) of `parent`'s children.
    pub fn key_at(&self, parent: Parent, index: Option<usize>, exclude: Option<NodeId>) -> String {
        let list: Vec<&(String, NodeId)> =
            self.child_keys(parent).iter().filter(|(_, id)| Some(*id) != exclude).collect();
        let i = index.unwrap_or(list.len()).min(list.len());
        let before = i.checked_sub(1).map(|j| list[j].0.as_str());
        let after = list.get(i).map(|e| e.0.as_str());
        match (before, after) {
            // Equal keys can only come from concurrent sites; append after both.
            (Some(a), Some(b)) if a >= b => key_between(Some(b), None),
            _ => key_between(before, after),
        }
    }

    fn check_parent(&self, parent: Parent, kind: NodeKind) -> Result<(), OpError> {
        match parent {
            Parent::Page(p) => match self.page(p) {
                None => Err(OpError::NoPage(p)),
                Some(pg) if pg.divider => invalid(format!("{p} is a divider and has no canvas")),
                Some(_) if kind == NodeKind::Vector => invalid("vector nodes must be inside an svg"),
                Some(_) => Ok(()),
            },
            Parent::Node(n) => {
                let pn = self.node(n).ok_or(OpError::NoNode(n))?;
                if kind.fits_in(pn.kind) {
                    Ok(())
                } else {
                    invalid(format!("a {kind:?} node can't go inside a {:?} node", pn.kind).to_lowercase())
                }
            }
        }
    }

    fn link(&mut self, id: NodeId, parent: Parent, key: String) {
        let list = self.children.entry(parent).or_default();
        let entry = (key.clone(), id);
        let at = list.binary_search(&entry).unwrap_or_else(|e| e);
        list.insert(at, entry);
        self.tree.insert(id, (parent, key));
        self.dirty.nodes.insert(id);
    }

    fn unlink(&mut self, id: NodeId) -> Option<(Parent, String)> {
        let (parent, key) = self.tree.remove(&id)?;
        if let Some(list) = self.children.get_mut(&parent)
            && let Ok(at) = list.binary_search(&(key.clone(), id))
        {
            list.remove(at);
        }
        self.dirty.nodes.insert(id);
        Some((parent, key))
    }

    fn page_index(&self, id: PageId) -> Result<usize, OpError> {
        self.pages.iter().position(|p| p.id == id).ok_or(OpError::NoPage(id))
    }

    /// Validates and applies one op, returning its inverse (None for ops that can't be undone).
    pub fn apply(&mut self, op: Op) -> Result<Option<Op>, OpError> {
        match op {
            Op::Insert { mut node, parent, key } => {
                if !is_valid_key(&key) {
                    return invalid(format!("invalid sort key {key:?}"));
                }
                if self.nodes.get(&node.id).is_some_and(|n| !n.deleted) {
                    return Err(OpError::Exists(node.id));
                }
                self.check_parent(parent, node.kind)?;
                if self.depth_below(parent) > MAX_DEPTH {
                    return invalid(format!("layers can't nest more than {MAX_DEPTH} levels deep"));
                }
                if node.kind == NodeKind::Vector && node.tag.as_deref().is_none_or(str::is_empty) {
                    return invalid("vector nodes need a tag");
                }
                if node.id.site == self.site && node.id.counter >= self.next_node {
                    self.next_node = node.id.counter + 1;
                    self.dirty.meta = true;
                }
                let id = node.id;
                node.deleted = false;
                self.nodes.insert(id, node);
                self.link(id, parent, key);
                Ok(Some(Op::Delete { id }))
            }
            Op::Delete { id } => {
                if self.node(id).is_none() {
                    return Err(OpError::NoNode(id));
                }
                let (parent, key) = self.unlink(id).ok_or(OpError::NoNode(id))?;
                let node = self.nodes.get_mut(&id).unwrap();
                node.deleted = true;
                let mut restored = node.clone();
                restored.deleted = false;
                Ok(Some(Op::Insert { node: restored, parent, key }))
            }
            Op::Move { id, parent, key } => {
                let kind = self.node(id).ok_or(OpError::NoNode(id))?.kind;
                if !is_valid_key(&key) {
                    return invalid(format!("invalid sort key {key:?}"));
                }
                self.check_parent(parent, kind)?;
                if let Parent::Node(p) = parent
                    && (p == id || self.ancestors(p).contains(&id))
                {
                    return invalid(format!("can't move {id} inside itself"));
                }
                let (from, to) = (self.parent(id).map_or(0, |p| self.depth_below(p)), self.depth_below(parent));
                if to > from {
                    let base = self.ancestors(id).len();
                    let height =
                        self.descendants(Parent::Node(id)).iter().map(|d| self.ancestors(*d).len() - base).max();
                    if to + height.unwrap_or(0) > MAX_DEPTH {
                        return invalid(format!("layers can't nest more than {MAX_DEPTH} levels deep"));
                    }
                }
                let (old_parent, old_key) = self.unlink(id).ok_or(OpError::NoNode(id))?;
                self.link(id, parent, key);
                Ok(Some(Op::Move { id, parent: old_parent, key: old_key }))
            }
            Op::Set { id, prop, value } => {
                let node = self.nodes.get_mut(&id).filter(|n| !n.deleted).ok_or(OpError::NoNode(id))?;
                let old = set_node_prop(node, &prop, value)?;
                self.dirty.nodes.insert(id);
                Ok(Some(Op::Set { id, prop, value: old }))
            }
            Op::InsertPage { page, index } => {
                if self.page(page.id).is_some() {
                    return Err(OpError::PageExists(page.id));
                }
                if page.name.trim().is_empty() && !page.divider {
                    return invalid("page name can't be empty");
                }
                if page.id.0 >= self.next_page {
                    self.next_page = page.id.0 + 1;
                    self.dirty.meta = true;
                }
                let id = page.id;
                self.pages.insert(index.min(self.pages.len()), page);
                self.dirty.pages = true;
                Ok(Some(Op::DeletePage { id }))
            }
            Op::DeletePage { id } => {
                let index = self.page_index(id)?;
                if !self.pages[index].divider && self.pages.iter().filter(|p| !p.divider).count() == 1 {
                    return invalid("a file needs at least one page");
                }
                let page = self.pages.remove(index);
                self.dirty.pages = true;
                Ok(Some(Op::InsertPage { page, index }))
            }
            Op::SetPage { id, prop, value } => {
                let index = self.page_index(id)?;
                let page = &mut self.pages[index];
                let old = match prop.as_str() {
                    "name" => {
                        let name = value.as_str().map(str::trim).filter(|s| !s.is_empty());
                        let name =
                            name.ok_or_else(|| OpError::Invalid("page name must be a non-empty string".into()))?;
                        Value::String(std::mem::replace(&mut page.name, name.chars().take(500).collect()))
                    }
                    "icon" => {
                        let icon = match &value {
                            Value::Null => None,
                            Value::String(s) if PAGE_ICONS.contains(&s.as_str()) => Some(s.clone()),
                            v => return invalid(format!("unknown page icon {v}")),
                        };
                        std::mem::replace(&mut page.icon, icon).map_or(Value::Null, Value::String)
                    }
                    "canvas" => {
                        let c = match &value {
                            Value::Null => None,
                            Value::String(s) => {
                                Some(s.parse().map_err(|e: crate::color::ColorError| OpError::Invalid(e.to_string()))?)
                            }
                            v => return invalid(format!("invalid canvas color {v}")),
                        };
                        std::mem::replace(&mut page.canvas, c).map_or(Value::Null, |c| Value::String(c.to_string()))
                    }
                    _ => return invalid(format!("unknown page property `{prop}`")),
                };
                self.dirty.pages = true;
                Ok(Some(Op::SetPage { id, prop, value: old }))
            }
            Op::MovePage { id, index } => {
                let from = self.page_index(id)?;
                let page = self.pages.remove(from);
                self.pages.insert(index.min(self.pages.len()), page);
                self.dirty.pages = true;
                Ok(Some(Op::MovePage { id, index: from }))
            }
            Op::SetToken { name, token } => {
                if let Some(t) = &token {
                    validate_name(&t.name).map_err(OpError::Invalid)?;
                    if t.name != name {
                        return invalid("token name mismatch");
                    }
                    if t.value.trim().is_empty() {
                        return invalid(format!("token `{name}` needs a value"));
                    }
                }
                let old = match token {
                    Some(t) => self.tokens.insert(name.clone(), t),
                    None => self.tokens.remove(&name),
                };
                self.dirty.tokens.insert(name.clone());
                Ok(Some(Op::SetToken { name, token: old }))
            }
            Op::AddAsset { hash, mime, bytes } => {
                if !self.assets.contains_key(&hash) {
                    self.assets.insert(hash.clone(), Asset { mime, bytes });
                    self.dirty.assets.insert(hash);
                }
                Ok(None)
            }
            Op::SetFileName { name } => {
                let name = name.trim().chars().take(500).collect::<String>();
                if name.is_empty() {
                    return invalid("file name can't be empty");
                }
                let old = std::mem::replace(&mut self.name, name);
                self.dirty.meta = true;
                Ok(Some(Op::SetFileName { name: old }))
            }
            Op::SetGuide { text } => {
                if text.len() > MAX_GUIDE_BYTES {
                    return invalid(format!("the style guide is limited to {} KB", MAX_GUIDE_BYTES / 1000));
                }
                let old = std::mem::replace(&mut self.guide, text);
                self.dirty.meta = true;
                Ok(Some(Op::SetGuide { text: old }))
            }
        }
    }
}

fn set_node_prop(node: &mut Node, prop: &str, value: Value) -> Result<Value, OpError> {
    let bad = |e: serde_json::Error| OpError::Invalid(format!("invalid value for `{prop}`: {e}"));
    Ok(match prop {
        "name" => {
            let name = value.as_str().ok_or_else(|| OpError::Invalid("name must be a string".into()))?;
            Value::String(std::mem::replace(&mut node.name, name.chars().take(200).collect()))
        }
        "hidden" => Value::Bool(std::mem::replace(&mut node.hidden, serde_json::from_value(value).map_err(bad)?)),
        "locked" => Value::Bool(std::mem::replace(&mut node.locked, serde_json::from_value(value).map_err(bad)?)),
        "text" => {
            if node.kind != NodeKind::Text {
                return invalid(format!("{} is not a text node", node.id));
            }
            let text: String = serde_json::from_value(value).map_err(bad)?;
            serde_json::to_value(node.text.replace(text)).unwrap()
        }
        "tag" => {
            if node.kind != NodeKind::Vector {
                return invalid(format!("{} is not a vector node", node.id));
            }
            let tag: String = serde_json::from_value(value).map_err(bad)?;
            if tag.is_empty() || !tag.chars().all(|c| c.is_ascii_alphanumeric()) {
                return invalid(format!("invalid svg tag `{tag}`"));
            }
            serde_json::to_value(node.tag.replace(tag)).unwrap()
        }
        "attrs" => serde_json::to_value(std::mem::replace(
            &mut node.attrs,
            if value.is_null() { Default::default() } else { serde_json::from_value(value).map_err(bad)? },
        ))
        .unwrap(),
        "exports" => serde_json::to_value(std::mem::replace(
            &mut node.exports,
            if value.is_null() { Vec::new() } else { serde_json::from_value(value).map_err(bad)? },
        ))
        .unwrap(),
        "layoutGuides" => {
            if node.kind != NodeKind::Frame {
                return invalid(format!("{} is not a frame", node.id));
            }
            let guides: Vec<LayoutGuide> =
                if value.is_null() { Vec::new() } else { serde_json::from_value(value).map_err(bad)? };
            if let Some(e) = guides.iter().find_map(|g| g.check().err()) {
                return invalid(e);
            }
            serde_json::to_value(std::mem::replace(&mut node.layout_guides, guides)).unwrap()
        }
        "image" => std::mem::replace(&mut node.image, (!value.is_null()).then_some(value)).unwrap_or(Value::Null),
        "style" => serde_json::to_value(std::mem::replace(
            &mut node.style,
            if value.is_null() { Default::default() } else { serde_json::from_value(value).map_err(bad)? },
        ))
        .unwrap(),
        p => match p.strip_prefix("style.") {
            Some(field) => node.style.set_field(field, value).map_err(|e| OpError::Invalid(e.to_string()))?,
            None => return invalid(format!("unknown node property `{prop}`")),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn doc_with_artboard() -> (Document, PageId, NodeId) {
        let mut doc = Document::new("Test");
        let page = doc.pages[0].id;
        let id = doc.alloc_node();
        let key = doc.key_at(Parent::Page(page), None, None);
        doc.apply(Op::Insert { node: Node::new(id, NodeKind::Frame), parent: Parent::Page(page), key }).unwrap();
        (doc, page, id)
    }

    fn add(doc: &mut Document, parent: NodeId, kind: NodeKind) -> NodeId {
        let id = doc.alloc_node();
        let key = doc.key_at(Parent::Node(parent), None, None);
        doc.apply(Op::Insert { node: Node::new(id, kind), parent: Parent::Node(parent), key }).unwrap();
        id
    }

    #[test]
    fn insert_delete_and_restore() {
        let (mut doc, page, board) = doc_with_artboard();
        let child = add(&mut doc, board, NodeKind::Text);
        assert_eq!(doc.node_count(), 2);
        assert_eq!(doc.page_of(child), Some(page));
        let inv = doc.apply(Op::Delete { id: board }).unwrap().unwrap();
        assert_eq!(doc.node_count(), 0);
        assert!(!doc.is_live(child), "children of a deleted node are gone too");
        assert!(doc.nodes.get(&board).unwrap().deleted, "tombstone kept");
        doc.apply(inv).unwrap();
        assert_eq!(doc.node_count(), 2);
        assert_eq!(doc.page_of(child), Some(page));
    }

    #[test]
    fn layout_guides_set_validate_and_undo() {
        let (mut doc, _, board) = doc_with_artboard();
        let text = add(&mut doc, board, NodeKind::Text);
        let cols = json!([{"kind": "columns", "count": 12, "gutter": 16, "margin": 24}, {"kind": "grid", "size": 8}]);
        let inv = doc.apply(Op::Set { id: board, prop: "layoutGuides".into(), value: cols.clone() }).unwrap().unwrap();
        assert_eq!(doc.node(board).unwrap().layout_guides.len(), 2);
        let saved = serde_json::to_value(doc.node(board).unwrap()).unwrap();
        assert_eq!(saved["layoutGuides"][0]["count"], 12, "stored on the node, so the file keeps them");
        assert!(serde_json::from_value::<Node>(saved).unwrap().layout_guides == doc.node(board).unwrap().layout_guides);
        doc.apply(inv).unwrap();
        assert!(doc.node(board).unwrap().layout_guides.is_empty());
        for bad in [
            json!([{"kind": "columns", "count": 0}]),
            json!([{"kind": "grid", "size": 0}]),
            json!([{"kind": "rows", "count": 2, "gutter": -1}]),
        ] {
            assert!(doc.apply(Op::Set { id: board, prop: "layoutGuides".into(), value: bad }).is_err());
        }
        assert!(doc.apply(Op::Set { id: text, prop: "layoutGuides".into(), value: cols }).is_err(), "frames only");
    }

    #[test]
    fn insert_validates_parent_and_kind() {
        let (mut doc, page, board) = doc_with_artboard();
        let text = add(&mut doc, board, NodeKind::Text);
        let id = doc.alloc_node();
        let err =
            doc.apply(Op::Insert { node: Node::new(id, NodeKind::Frame), parent: Parent::Node(text), key: "P".into() });
        assert!(matches!(err, Err(OpError::Invalid(_))));
        let err = doc.apply(Op::Insert {
            node: Node::new(board, NodeKind::Frame),
            parent: Parent::Page(page),
            key: "Q".into(),
        });
        assert_eq!(err, Err(OpError::Exists(board)));
        let err = doc.apply(Op::Insert {
            node: Node::new(id, NodeKind::Frame),
            parent: Parent::Page(PageId(99)),
            key: "Q".into(),
        });
        assert_eq!(err, Err(OpError::NoPage(PageId(99))));
        let err =
            doc.apply(Op::Insert { node: Node::new(id, NodeKind::Frame), parent: Parent::Page(page), key: "".into() });
        assert!(err.is_err());
    }

    #[test]
    fn move_reorders_reparents_and_rejects_cycles() {
        let (mut doc, page, board) = doc_with_artboard();
        let a = add(&mut doc, board, NodeKind::Frame);
        let b = add(&mut doc, board, NodeKind::Rect);
        assert_eq!(doc.children(Parent::Node(board)).collect::<Vec<_>>(), vec![a, b]);
        let key = doc.key_at(Parent::Node(board), Some(0), Some(b));
        let inv = doc.apply(Op::Move { id: b, parent: Parent::Node(board), key }).unwrap().unwrap();
        assert_eq!(doc.children(Parent::Node(board)).collect::<Vec<_>>(), vec![b, a]);
        doc.apply(inv).unwrap();
        assert_eq!(doc.children(Parent::Node(board)).collect::<Vec<_>>(), vec![a, b]);
        let err = doc.apply(Op::Move { id: board, parent: Parent::Node(a), key: "P".into() });
        assert!(matches!(err, Err(OpError::Invalid(_))));
        let key = doc.key_at(Parent::Page(page), None, None);
        doc.apply(Op::Move { id: a, parent: Parent::Page(page), key }).unwrap();
        assert_eq!(doc.artboard_of(a), Some(a));
    }

    #[test]
    fn set_properties_with_inverse() {
        let (mut doc, _, board) = doc_with_artboard();
        let inv = doc.apply(Op::Set { id: board, prop: "style.width".into(), value: json!("200px") }).unwrap().unwrap();
        assert_eq!(inv, Op::Set { id: board, prop: "style.width".into(), value: json!(null) });
        doc.apply(Op::Set { id: board, prop: "name".into(), value: json!("Hero") }).unwrap();
        assert_eq!(doc.node(board).unwrap().name, "Hero");
        assert!(doc.apply(Op::Set { id: board, prop: "text".into(), value: json!("x") }).is_err());
        assert!(doc.apply(Op::Set { id: board, prop: "style.nope".into(), value: json!(1) }).is_err());
        assert!(doc.apply(Op::Set { id: board, prop: "bogus".into(), value: json!(1) }).is_err());
    }

    #[test]
    fn page_ops() {
        let mut doc = Document::new("Pages");
        let first = doc.pages[0].id;
        assert!(doc.apply(Op::DeletePage { id: first }).is_err(), "last page stays");
        let id = doc.alloc_page();
        let page = Page { id, name: "Second".into(), icon: None, canvas: None, divider: false };
        doc.apply(Op::InsertPage { page, index: 1 }).unwrap();
        let inv = doc.apply(Op::MovePage { id, index: 0 }).unwrap().unwrap();
        assert_eq!(doc.pages[0].id, id);
        doc.apply(inv).unwrap();
        assert_eq!(doc.pages[1].id, id);
        doc.apply(Op::SetPage { id, prop: "icon".into(), value: json!("check") }).unwrap();
        assert!(doc.apply(Op::SetPage { id, prop: "icon".into(), value: json!("rocket") }).is_err());
        doc.apply(Op::SetPage { id, prop: "canvas".into(), value: json!("#222") }).unwrap();
        let inv = doc.apply(Op::DeletePage { id }).unwrap().unwrap();
        assert_eq!(doc.pages.len(), 1);
        doc.apply(inv).unwrap();
        assert_eq!(doc.page(id).unwrap().icon.as_deref(), Some("check"));
    }

    #[test]
    fn guide_edits_undo_and_stay_bounded() {
        let mut doc = Document::new("Guide");
        let inv = doc.apply(Op::SetGuide { text: "- Flat".into() }).unwrap().unwrap();
        assert_eq!(doc.guide, "- Flat");
        doc.apply(inv).unwrap();
        assert_eq!(doc.guide, "");
        assert!(doc.apply(Op::SetGuide { text: "x".repeat(MAX_GUIDE_BYTES + 1) }).is_err());
    }

    #[test]
    fn token_ops_validate() {
        let mut doc = Document::new("Tokens");
        let t = Token { name: "--color-brand".into(), value: "#000".into(), description: None };
        let inv = doc.apply(Op::SetToken { name: t.name.clone(), token: Some(t) }).unwrap().unwrap();
        assert_eq!(doc.tokens.len(), 1);
        doc.apply(inv).unwrap();
        assert!(doc.tokens.is_empty());
        let bad = Token { name: "--wat".into(), value: "1".into(), description: None };
        assert!(doc.apply(Op::SetToken { name: "--wat".into(), token: Some(bad) }).is_err());
    }

    #[test]
    fn ops_serialize_as_tagged_json() {
        let op = Op::Set { id: NodeId::new(5, 0), prop: "name".into(), value: json!("A") };
        let s = serde_json::to_string(&op).unwrap();
        assert_eq!(s, r#"{"op":"set","id":"5-0","prop":"name","value":"A"}"#);
        assert_eq!(serde_json::from_str::<Op>(&s).unwrap(), op);
    }
}
