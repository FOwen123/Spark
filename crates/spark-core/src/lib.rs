//! Spark's document core: ids, nodes, styles, ops, undo, persistence and code export.

pub mod asset;
pub mod codegen;
pub mod color;
pub mod css;
pub mod demo;
pub mod doc;
pub mod export;
pub mod fractional;
pub mod history;
pub mod html;
pub mod id;
pub mod node;
pub mod store;
pub mod style;
pub mod svg;
pub mod tokens;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub use doc::{Document, Op, OpError, Parent, Rect};
pub use id::{NodeId, PageId};
pub use node::{GuideKind, ImageStatus, LayoutGuide, Node, NodeKind, Page};
pub use style::Style;
