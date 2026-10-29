//! Performance budgets from the spec: open a 20,000-node file in under 1 s and apply an edit op in
//! under 1 ms.

use std::time::Instant;

use serde_json::json;
use spark_core::{Op, Parent, demo::demo_document, history::Editor, store::Store};

#[test]
fn opens_a_20k_node_file_under_one_second() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("big.spark");
    let mut doc = demo_document(20_000);
    assert!(doc.node_count() >= 20_000);
    Store::create(&path, &mut doc).unwrap();
    let started = Instant::now();
    let (_, loaded) = Store::open(&path).unwrap();
    let elapsed = started.elapsed();
    assert!(loaded.node_count() >= 20_000);
    println!("open 20k nodes: {elapsed:?}");
    assert!(elapsed.as_secs_f64() < 1.0, "open took {elapsed:?}");
}

#[test]
fn applies_an_edit_op_under_one_millisecond() {
    let mut ed = Editor::new(demo_document(20_000));
    let page = ed.doc.pages[0].id;
    let board = ed.doc.children(Parent::Page(page)).next().unwrap();
    let card = ed.doc.children(Parent::Node(board)).nth(1).unwrap();
    let n = 1000;
    let started = Instant::now();
    for i in 0..n {
        ed.transact(vec![Op::Set { id: card, prop: "style.gap".into(), value: json!(format!("{i}px")) }], None)
            .unwrap();
    }
    let per_op = started.elapsed() / n;
    println!("set op: {per_op:?}");
    assert!(per_op.as_secs_f64() < 0.001, "op took {per_op:?}");
}
