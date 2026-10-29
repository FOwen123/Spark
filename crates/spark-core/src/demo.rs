//! Generated demo content for tests, benchmarks and screenshots. No real user data.

use serde_json::json;

use crate::{
    doc::{Document, Op, Parent},
    fractional::keys_between,
    node::{Node, NodeKind},
};

const TITLES: &[&str] = &["Overview", "Pricing", "Team", "Features", "Changelog", "Contact", "Gallery", "Settings"];
const BODY: &str = "Spark keeps designs as real HTML and CSS, so what you draw is what ships.";

fn node(doc: &mut Document, kind: NodeKind, name: &str, css: serde_json::Value) -> Node {
    let mut n = Node::new(doc.alloc_node(), kind);
    n.name = name.into();
    n.style = serde_json::from_value(css).expect("demo style");
    n
}

/// A document with roughly `target` nodes: artboards in a grid, each holding a column of cards.
pub fn demo_document(target: usize) -> Document {
    let mut doc = Document::new("Demo");
    let page = doc.pages[0].id;
    // Each card is 4 nodes (card, title, body, swatch); each board adds itself and a heading.
    let cards_per_board = 24usize;
    let per_board = 2 + cards_per_board * 4;
    let boards = target.div_ceil(per_board).max(1);
    let board_keys = keys_between(None, None, boards);
    let mut ops = Vec::new();
    for (b, bkey) in board_keys.into_iter().enumerate() {
        let (col, row) = (b % 10, b / 10);
        let mut board = node(
            &mut doc,
            NodeKind::Frame,
            &format!("{} {}", TITLES[b % TITLES.len()], b + 1),
            json!({
                "position": "absolute", "left": format!("{}px", col * 1500), "top": format!("{}px", row * 2400),
                "width": "1440px", "height": "hug", "display": "flex", "flexDirection": "column",
                "gap": "16px", "padding": ["48px", "48px", "48px", "48px"], "overflow": "clip",
                "fills": [{"type": "solid", "color": "#ffffff"}],
            }),
        );
        board.style.font_family = Some("Geist, sans-serif".into());
        let bid = board.id;
        ops.push(Op::Insert { node: board, parent: Parent::Page(page), key: bkey });
        let mut keys = keys_between(None, None, cards_per_board + 1).into_iter();
        let mut heading = node(
            &mut doc,
            NodeKind::Text,
            "Heading",
            json!({"fontSize": "48px", "fontWeight": 600, "color": "#0a0a0a"}),
        );
        heading.text = Some(TITLES[b % TITLES.len()].into());
        ops.push(Op::Insert { node: heading, parent: Parent::Node(bid), key: keys.next().unwrap() });
        for c in 0..cards_per_board {
            let card = node(
                &mut doc,
                NodeKind::Frame,
                "Card",
                json!({
                    "width": "fill", "height": "hug", "display": "flex", "flexDirection": "row", "alignItems": "center",
                    "gap": "16px", "padding": ["16px", "20px", "16px", "20px"], "radius": ["12px", "12px", "12px", "12px"],
                    "fills": [{"type": "solid", "color": "#f4f4f5"}],
                    "border": {"width": [1.0, 1.0, 1.0, 1.0], "style": "solid", "color": "#e4e4e7"},
                }),
            );
            let cid = card.id;
            ops.push(Op::Insert { node: card, parent: Parent::Node(bid), key: keys.next().unwrap() });
            let shade = 40 + (b * 7 + c * 13) % 50;
            let swatch = node(
                &mut doc,
                NodeKind::Rect,
                "Swatch",
                json!({"width": "48px", "height": "48px", "radius": ["8px", "8px", "8px", "8px"],
                       "fills": [{"type": "linear", "angle": 135.0, "stops": [
                           {"color": format!("oklch(0.{shade} 0.12 {})", (c * 37) % 360), "pos": 0.0},
                           {"color": "#0a0a0a", "pos": 1.0}]}]}),
            );
            let mut title = node(
                &mut doc,
                NodeKind::Text,
                "Title",
                json!({"fontSize": "18px", "fontWeight": 600, "color": "#18181b"}),
            );
            title.text = Some(format!("Item {}", c + 1));
            let mut body = node(
                &mut doc,
                NodeKind::Text,
                "Body",
                json!({"width": "fill", "fontSize": "14px", "lineHeight": "1.5", "color": "#52525b"}),
            );
            body.text = Some(BODY.into());
            for (n, k) in [swatch, title, body].into_iter().zip(keys_between(None, None, 3)) {
                ops.push(Op::Insert { node: n, parent: Parent::Node(cid), key: k });
            }
        }
    }
    for op in ops {
        doc.apply(op).expect("demo op");
    }
    doc
}
