//! Nodes and pages.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    color::Color,
    id::{NodeId, PageId},
    style::Style,
};

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    /// A box that can hold children, with freeform or flex layout.
    Frame,
    /// A box without children.
    Rect,
    /// Plain text with one style.
    Text,
    /// An `<svg>` element; its children are vectors.
    Svg,
    /// An SVG child element (`path`, `circle`, `g`, …) named by `tag`.
    Vector,
}

impl NodeKind {
    pub fn default_name(self) -> &'static str {
        match self {
            NodeKind::Frame => "Frame",
            NodeKind::Rect => "Rectangle",
            NodeKind::Text => "Text",
            NodeKind::Svg => "SVG",
            NodeKind::Vector => "Vector",
        }
    }

    /// Whether a node of this kind may sit directly inside `parent`.
    pub fn fits_in(self, parent: NodeKind) -> bool {
        match parent {
            NodeKind::Frame => self != NodeKind::Vector,
            NodeKind::Svg | NodeKind::Vector => self == NodeKind::Vector,
            NodeKind::Rect | NodeKind::Text => false,
        }
    }
}

/// A saved export setting on a node: format plus scale (`2x`, `512w`, `512h`).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportPreset {
    pub format: String,
    pub scale: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
    /// JPEG quality, 1..=100 (default 90).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GuideKind {
    Columns,
    Rows,
    /// Square cells of `size` px.
    Grid,
}

/// A layout guide drawn over a frame on the canvas (never rendered into exports).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayoutGuide {
    pub kind: GuideKind,
    /// Number of columns or rows (columns and rows guides).
    #[serde(default)]
    pub count: u32,
    /// Space between columns or rows.
    #[serde(default)]
    pub gutter: f64,
    /// Space between the frame's edge and the first and last column or row.
    #[serde(default)]
    pub margin: f64,
    /// Cell size (grid guides).
    #[serde(default)]
    pub size: f64,
}

impl LayoutGuide {
    /// Guides need a positive count (columns, rows) or size (grid) and non-negative spacing.
    pub fn check(&self) -> Result<(), String> {
        let ok = |v: f64| v.is_finite() && (0.0..=100_000.0).contains(&v);
        let shape = match self.kind {
            GuideKind::Grid => ok(self.size) && self.size >= 1.0,
            _ => (1..=500).contains(&self.count),
        };
        if shape && ok(self.gutter) && ok(self.margin) {
            Ok(())
        } else {
            Err("a layout guide needs a count of 1-500 (columns, rows) or a size of at least 1 (grid), and spacing of 0 or more".into())
        }
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub locked: bool,
    /// Text content (text nodes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// SVG tag (vector nodes).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// SVG attributes (`viewBox`, `d`, `fill`, …) for svg and vector nodes.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attrs: BTreeMap<String, String>,
    #[serde(default)]
    pub style: Style,
    /// The token theme this layer and its contents use (`data-theme`); none uses the base values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exports: Vec<ExportPreset>,
    /// Column, row and grid guides drawn over a frame.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layout_guides: Vec<LayoutGuide>,
    /// Image request or generation state (see the Images section of the spec); `ImageStatus` reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<serde_json::Value>,
    /// Tombstone: deleted nodes stay in the file so undo and future sync can restore them.
    #[serde(default, skip_serializing_if = "is_false")]
    pub deleted: bool,
}

impl Node {
    pub fn new(id: NodeId, kind: NodeKind) -> Node {
        Node {
            id,
            kind,
            name: kind.default_name().to_string(),
            hidden: false,
            locked: false,
            text: (kind == NodeKind::Text).then(String::new),
            tag: None,
            attrs: BTreeMap::new(),
            style: Style::default(),
            theme: None,
            exports: Vec::new(),
            layout_guides: Vec::new(),
            image: None,
            deleted: false,
        }
    }
}

#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub id: PageId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canvas: Option<Color>,
    /// A divider is a page-list entry with no canvas.
    #[serde(default, skip_serializing_if = "is_false")]
    pub divider: bool,
}

pub const PAGE_ICONS: &[&str] = &[
    "arrow",
    "padlock",
    "check",
    "cross",
    "info",
    "question",
    "design",
    "vector",
    "images",
    "table",
    "presentation",
    "blueprint",
    "swatches",
    "bar-chart",
    "line-chart",
    "pie-chart",
    "page",
];

/// Where a requested or generated image stands, stored as `status` in a node's `image`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageStatus {
    /// Waiting for an agent to supply the image.
    Pending,
    /// A provider job is generating it. The job lives in the running app, not in the file.
    Processing,
    Ready,
    Error,
}

impl ImageStatus {
    pub fn of(image: &serde_json::Value) -> ImageStatus {
        match image["status"].as_str() {
            Some("pending") => ImageStatus::Pending,
            Some("ready") => ImageStatus::Ready,
            Some("error") => ImageStatus::Error,
            _ => ImageStatus::Processing,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ImageStatus::Pending => "pending",
            ImageStatus::Processing => "processing",
            ImageStatus::Ready => "ready",
            ImageStatus::Error => "error",
        }
    }
}
