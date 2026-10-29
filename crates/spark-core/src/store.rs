//! `.spark` files: one SQLite database per document, saved incrementally in WAL mode.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{Connection, OptionalExtension, params};

use crate::{
    doc::{Asset, Document, Op, Parent, Rect},
    id::NodeId,
    node::{Node, Page},
    tokens::Token,
};

/// Current schema version, stored in SQLite's `user_version`.
pub const SCHEMA: u32 = 1;

/// `MIGRATIONS[i]` upgrades a file from version `i` to `i + 1`.
const MIGRATIONS: &[&str] = &["
    CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
    CREATE TABLE pages (id TEXT PRIMARY KEY, idx INTEGER NOT NULL, json TEXT NOT NULL);
    CREATE TABLE nodes (id TEXT PRIMARY KEY, json TEXT NOT NULL);
    CREATE TABLE tree (id TEXT PRIMARY KEY, parent TEXT NOT NULL, key TEXT NOT NULL);
    CREATE INDEX tree_parent ON tree (parent, key);
    CREATE TABLE tokens (name TEXT PRIMARY KEY, json TEXT NOT NULL);
    CREATE TABLE assets (hash TEXT PRIMARY KEY, mime TEXT NOT NULL, bytes BLOB NOT NULL);
    CREATE TABLE ops (seq INTEGER PRIMARY KEY AUTOINCREMENT, ts INTEGER NOT NULL, json TEXT NOT NULL);
    CREATE TABLE layout (id TEXT PRIMARY KEY, x REAL NOT NULL, y REAL NOT NULL, w REAL NOT NULL, h REAL NOT NULL);
"];

/// The op log keeps the most recent transactions only.
const OPS_KEPT: i64 = 10_000;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("corrupt data: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Other(String),
}

impl From<rusqlite::types::FromSqlError> for StoreError {
    fn from(e: rusqlite::types::FromSqlError) -> Self {
        StoreError::Other(e.to_string())
    }
}

#[derive(Debug)]
pub struct Store {
    conn: Connection,
    path: PathBuf,
    /// Writes since the last checkpoint into the main file.
    unsynced: bool,
    /// Save count when this store last wrote; a different count on disk means another writer.
    rev: i64,
}

fn now_ms() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn connect(path: &Path) -> Result<Connection, StoreError> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    Ok(conn)
}

/// Runs pending migrations. Refuses files written by a newer Spark.
fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    let version: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version > SCHEMA {
        return Err(StoreError::Other(format!("this file uses schema {version}; update Spark to open it")));
    }
    let tx = conn.transaction()?;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i as u32 + 1)?;
    }
    tx.commit()?;
    Ok(())
}

impl Store {
    /// Creates a new `.spark` file and writes the whole document. Fails if the file exists.
    pub fn create(path: &Path, doc: &mut Document) -> Result<Store, StoreError> {
        if path.exists() {
            return Err(StoreError::Other(format!("{} already exists", path.display())));
        }
        let mut conn = connect(path)?;
        migrate(&mut conn)?;
        let mut store = Store { conn, path: path.to_path_buf(), unsynced: false, rev: 0 };
        doc.dirty.meta = true;
        doc.dirty.pages = true;
        doc.dirty.nodes = doc.nodes.keys().copied().collect();
        doc.dirty.tokens = doc.tokens.keys().cloned().collect();
        doc.dirty.assets = doc.assets.keys().cloned().collect();
        doc.dirty.layout = doc.measured.keys().copied().collect();
        store.save(doc, &[])?;
        Ok(store)
    }

    pub fn open(path: &Path) -> Result<(Store, Document), StoreError> {
        if !path.exists() {
            return Err(StoreError::Other(format!("{} does not exist", path.display())));
        }
        let mut conn = connect(path)?;
        migrate(&mut conn)?;
        let doc = load(&conn)?;
        let rev = rev(&conn)?;
        Ok((Store { conn, path: path.to_path_buf(), unsynced: false, rev }, doc))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Writes everything marked dirty, plus `ops` to the op log, in one atomic transaction.
    pub fn save(&mut self, doc: &mut Document, ops: &[Op]) -> Result<(), StoreError> {
        let dirty = std::mem::take(&mut doc.dirty);
        if dirty.is_empty() && ops.is_empty() {
            return Ok(());
        }
        match self.write(doc, &dirty, ops) {
            Ok(()) => Ok(()),
            Err(e) => {
                doc.dirty.merge(dirty);
                Err(e)
            }
        }
    }

    fn write(&mut self, doc: &Document, dirty: &crate::doc::Dirty, ops: &[Op]) -> Result<(), StoreError> {
        let tx = self.conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let rev = rev(&tx)?;
        if rev != self.rev {
            return Err(StoreError::Other(
                "another Spark saved this file since it was opened here; reopen it to keep editing".into(),
            ));
        }
        tx.execute("INSERT OR REPLACE INTO meta (key, value) VALUES ('rev', ?1)", params![(rev + 1).to_string()])?;
        if dirty.meta {
            let mut st = tx.prepare_cached("INSERT OR REPLACE INTO meta (key, value) VALUES (?1, ?2)")?;
            st.execute(params!["id", doc.id])?;
            st.execute(params!["name", doc.name])?;
            st.execute(params!["nextNode", doc.next_node.to_string()])?;
            st.execute(params!["nextPage", doc.next_page.to_string()])?;
            st.execute(params!["site", doc.site.to_string()])?;
        }
        if dirty.pages {
            tx.execute("DELETE FROM pages", [])?;
            let mut st = tx.prepare_cached("INSERT INTO pages (id, idx, json) VALUES (?1, ?2, ?3)")?;
            for (i, p) in doc.pages.iter().enumerate() {
                st.execute(params![p.id.to_string(), i as i64, serde_json::to_string(p)?])?;
            }
        }
        {
            let mut put_node = tx.prepare_cached("INSERT OR REPLACE INTO nodes (id, json) VALUES (?1, ?2)")?;
            let mut put_tree =
                tx.prepare_cached("INSERT OR REPLACE INTO tree (id, parent, key) VALUES (?1, ?2, ?3)")?;
            let mut del_tree = tx.prepare_cached("DELETE FROM tree WHERE id = ?1")?;
            for id in &dirty.nodes {
                let sid = id.to_string();
                if let Some(n) = doc.nodes.get(id) {
                    put_node.execute(params![sid, serde_json::to_string(n)?])?;
                }
                match doc.tree.get(id) {
                    Some((parent, key)) => put_tree.execute(params![sid, parent.to_string(), key])?,
                    None => del_tree.execute(params![sid])?,
                };
            }
        }
        for name in &dirty.tokens {
            match doc.tokens.get(name) {
                Some(t) => tx.execute(
                    "INSERT OR REPLACE INTO tokens (name, json) VALUES (?1, ?2)",
                    params![name, serde_json::to_string(t)?],
                )?,
                None => tx.execute("DELETE FROM tokens WHERE name = ?1", params![name])?,
            };
        }
        for hash in &dirty.assets {
            if let Some(a) = doc.assets.get(hash) {
                tx.execute(
                    "INSERT OR IGNORE INTO assets (hash, mime, bytes) VALUES (?1, ?2, ?3)",
                    params![hash, a.mime, a.bytes.as_slice()],
                )?;
            }
        }
        {
            let mut put =
                tx.prepare_cached("INSERT OR REPLACE INTO layout (id, x, y, w, h) VALUES (?1, ?2, ?3, ?4, ?5)")?;
            let mut del = tx.prepare_cached("DELETE FROM layout WHERE id = ?1")?;
            for id in &dirty.layout {
                match doc.measured.get(id) {
                    Some(r) => put.execute(params![id.to_string(), r.x, r.y, r.w, r.h])?,
                    None => del.execute(params![id.to_string()])?,
                };
            }
        }
        if !ops.is_empty() {
            tx.execute("INSERT INTO ops (ts, json) VALUES (?1, ?2)", params![now_ms(), serde_json::to_string(ops)?])?;
            tx.execute("DELETE FROM ops WHERE seq <= (SELECT MAX(seq) FROM ops) - ?1", params![OPS_KEPT])?;
        }
        tx.commit()?;
        self.rev = rev + 1;
        self.unsynced = true;
        Ok(())
    }

    /// Copies WAL contents into the main file so the `.spark` file is complete on its own (for
    /// copying, attaching or syncing). Cheap when nothing changed; call when edits go idle.
    pub fn checkpoint(&mut self) -> Result<(), StoreError> {
        if self.unsynced {
            let (busy, log, done): (i64, i64, i64) =
                self.conn
                    .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            // A reader on an older snapshot blocks it; stay pending so the next idle flush retries.
            self.unsynced = busy != 0 || log != done;
        }
        Ok(())
    }

    pub fn has_unsynced(&self) -> bool {
        self.unsynced
    }

    /// Number of transactions in the op log.
    pub fn op_log_len(&self) -> Result<i64, StoreError> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM ops", [], |r| r.get(0))?)
    }
}

fn rev(conn: &Connection) -> Result<i64, StoreError> {
    let v: Option<String> = conn.query_row("SELECT value FROM meta WHERE key = 'rev'", [], |r| r.get(0)).optional()?;
    Ok(v.and_then(|v| v.parse().ok()).unwrap_or(0))
}

fn load(conn: &Connection) -> Result<Document, StoreError> {
    let meta = |key: &str| -> Result<Option<String>, StoreError> {
        Ok(conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |r| r.get(0)).optional()?)
    };
    let id = meta("id")?.ok_or_else(|| StoreError::Other("not a Spark file (no id)".into()))?;
    let name = meta("name")?.unwrap_or_else(|| "Untitled".into());

    let mut st = conn.prepare("SELECT json FROM pages ORDER BY idx")?;
    let pages = st
        .query_map([], |r| r.get::<_, String>(0))?
        .map(|j| Ok(serde_json::from_str::<Page>(&j?)?))
        .collect::<Result<Vec<_>, StoreError>>()?;

    let mut nodes = HashMap::new();
    let mut st = conn.prepare("SELECT json FROM nodes")?;
    let mut rows = st.query([])?;
    while let Some(row) = rows.next()? {
        let n: Node = serde_json::from_str(row.get_ref(0)?.as_str()?)?;
        nodes.insert(n.id, n);
    }
    let mut tree = HashMap::new();
    let mut st = conn.prepare("SELECT id, parent, key FROM tree")?;
    let mut rows = st.query([])?;
    let bad = |e: crate::id::IdError| StoreError::Other(e.to_string());
    while let Some(row) = rows.next()? {
        let id: NodeId = row.get_ref(0)?.as_str()?.parse().map_err(bad)?;
        let parent: Parent = row.get_ref(1)?.as_str()?.parse().map_err(bad)?;
        tree.insert(id, (parent, row.get::<_, String>(2)?));
    }
    let mut doc = Document::from_parts(id, name, pages, nodes, tree);
    if let Some(n) = doc.too_deep() {
        return Err(StoreError::Other(format!(
            "layer {n} is nested more than {} levels deep or inside itself",
            crate::doc::MAX_DEPTH
        )));
    }
    if let Some(site) = meta("site")?.and_then(|s| s.parse().ok()) {
        doc.site = site;
    }
    if let Some(n) = meta("nextNode")?.and_then(|s| s.parse::<u64>().ok()) {
        doc.next_node = doc.next_node.max(n);
    }
    if let Some(n) = meta("nextPage")?.and_then(|s| s.parse::<u64>().ok()) {
        doc.next_page = doc.next_page.max(n);
    }

    let mut st = conn.prepare("SELECT json FROM tokens")?;
    for j in st.query_map([], |r| r.get::<_, String>(0))? {
        let t: Token = serde_json::from_str(&j?)?;
        doc.tokens.insert(t.name.clone(), t);
    }
    let mut st = conn.prepare("SELECT hash, mime, bytes FROM assets")?;
    for row in st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Vec<u8>>(2)?)))? {
        let (hash, mime, bytes) = row?;
        doc.assets.insert(hash, Asset { mime, bytes: Arc::new(bytes) });
    }
    let mut st = conn.prepare("SELECT id, x, y, w, h FROM layout")?;
    let mut rows = st.query([])?;
    while let Some(row) = rows.next()? {
        if let Ok(id) = row.get_ref(0)?.as_str()?.parse::<NodeId>() {
            doc.measured.insert(id, Rect { x: row.get(1)?, y: row.get(2)?, w: row.get(3)?, h: row.get(4)? });
        }
    }
    doc.dirty = Default::default();
    Ok(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{doc::Parent, history::Editor, node::NodeKind};
    use serde_json::json;

    fn sample() -> Document {
        let mut doc = Document::new("Sample");
        let page = doc.pages[0].id;
        let board = doc.alloc_node();
        let text = doc.alloc_node();
        let mut ab = Node::new(board, NodeKind::Frame);
        ab.style.set_field("width", json!("400px")).unwrap();
        ab.style.set_field("fills", json!([{"type": "solid", "color": "#ffffff"}])).unwrap();
        let mut t = Node::new(text, NodeKind::Text);
        t.text = Some("Hello".into());
        let k1 = doc.key_at(Parent::Page(page), None, None);
        doc.apply(Op::Insert { node: ab, parent: Parent::Page(page), key: k1 }).unwrap();
        let k2 = doc.key_at(Parent::Node(board), None, None);
        doc.apply(Op::Insert { node: t, parent: Parent::Node(board), key: k2 }).unwrap();
        let tok = Token { name: "--color-ink".into(), value: "#111".into(), description: Some("Body text".into()) };
        doc.apply(Op::SetToken { name: tok.name.clone(), token: Some(tok) }).unwrap();
        doc.apply(Op::AddAsset { hash: "abc".into(), mime: "image/png".into(), bytes: Arc::new(vec![1, 2, 3]) })
            .unwrap();
        doc.set_measured(board, Rect { x: 0.0, y: 0.0, w: 400.0, h: 300.0 });
        doc
    }

    #[test]
    fn round_trips_a_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.spark");
        let mut doc = sample();
        Store::create(&path, &mut doc).unwrap();
        let (_, loaded) = Store::open(&path).unwrap();
        assert_eq!(loaded.id, doc.id);
        assert_eq!(loaded.name, "Sample");
        assert_eq!(loaded.pages, doc.pages);
        assert_eq!(loaded.node_count(), 2);
        let board = doc.children(Parent::Page(doc.pages[0].id)).next().unwrap();
        assert_eq!(loaded.node(board), doc.node(board));
        assert_eq!(loaded.tokens, doc.tokens);
        assert_eq!(loaded.assets["abc"].bytes.as_slice(), &[1, 2, 3]);
        assert_eq!(loaded.measured.get(&board).unwrap().w, 400.0);
        assert_eq!(loaded.next_node, doc.next_node);
    }

    #[test]
    fn saves_incrementally_including_deletes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("b.spark");
        let mut ed = Editor::new(sample());
        let mut store = Store::create(&path, &mut ed.doc).unwrap();
        let page = ed.doc.pages[0].id;
        let board = ed.doc.children(Parent::Page(page)).next().unwrap();
        let ops = ed.transact(vec![Op::Set { id: board, prop: "name".into(), value: json!("Renamed") }], None).unwrap();
        assert_eq!(ed.doc.dirty.nodes.len(), 1, "only the changed node is dirty");
        store.save(&mut ed.doc, &ops).unwrap();
        let ops = ed.transact(vec![Op::Delete { id: board }], None).unwrap();
        store.save(&mut ed.doc, &ops).unwrap();
        assert_eq!(store.op_log_len().unwrap(), 2);
        let (_, loaded) = Store::open(&path).unwrap();
        assert_eq!(loaded.node_count(), 0);
        assert!(loaded.all_nodes().any(|n| n.id == board && n.deleted && n.name == "Renamed"));
        ed.undo();
        store.save(&mut ed.doc, &[]).unwrap();
        let (_, loaded) = Store::open(&path).unwrap();
        assert_eq!(loaded.node_count(), 2);
    }

    #[test]
    fn refuses_newer_schema_and_existing_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.spark");
        let mut doc = sample();
        drop(Store::create(&path, &mut doc).unwrap());
        assert!(Store::create(&path, &mut doc).is_err());
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", SCHEMA + 1).unwrap();
        drop(conn);
        assert!(Store::open(&path).unwrap_err().to_string().contains("update Spark"));
    }

    #[test]
    fn migrates_an_unversioned_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("d.spark");
        let mut conn = Connection::open(&path).unwrap();
        migrate(&mut conn).unwrap();
        assert_eq!(conn.pragma_query_value(None, "user_version", |r| r.get::<_, u32>(0)).unwrap(), SCHEMA);
        migrate(&mut conn).unwrap();
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = self.checkpoint();
    }
}

#[cfg(test)]
mod checkpoint_tests {
    use super::*;

    #[test]
    fn checkpoint_makes_the_main_file_complete() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.spark");
        let mut doc = crate::demo::demo_document(300);
        let mut store = Store::create(&path, &mut doc).unwrap();
        store.checkpoint().unwrap();
        // A copy of just the .spark file (no -wal) opens with everything in it.
        let copy = dir.path().join("copy.spark");
        std::fs::copy(&path, &copy).unwrap();
        let (_, loaded) = Store::open(&copy).unwrap();
        assert_eq!(loaded.node_count(), doc.node_count());
        drop(store);
    }
}

#[cfg(test)]
mod writer_tests {
    use super::*;

    fn rename(store: &mut Store, doc: &mut Document, name: &str) -> Result<(), StoreError> {
        doc.apply(Op::SetFileName { name: name.into() }).unwrap();
        store.save(doc, &[])
    }

    #[test]
    fn a_checkpoint_blocked_by_a_reader_is_retried() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("c.spark");
        let mut doc = Document::new("Before");
        let mut store = Store::create(&path, &mut doc).unwrap();
        store.checkpoint().unwrap();
        let mut reader = Connection::open(&path).unwrap();
        let read = reader.transaction().unwrap();
        read.query_row("SELECT COUNT(*) FROM nodes", [], |r| r.get::<_, i64>(0)).unwrap();
        rename(&mut store, &mut doc, "After").unwrap();
        store.checkpoint().unwrap();
        assert!(store.has_unsynced(), "a blocked checkpoint stays pending");
        drop(read);
        store.checkpoint().unwrap();
        assert!(!store.has_unsynced());
        let copy = dir.path().join("copy.spark");
        std::fs::copy(&path, &copy).unwrap();
        assert_eq!(Store::open(&copy).unwrap().1.name, "After");
    }

    #[test]
    fn a_second_writer_on_one_file_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("w.spark");
        Store::create(&path, &mut Document::new("Start")).unwrap();
        let (mut a, mut doc_a) = Store::open(&path).unwrap();
        let (mut b, mut doc_b) = Store::open(&path).unwrap();
        rename(&mut a, &mut doc_a, "From A").unwrap();
        assert!(rename(&mut b, &mut doc_b, "From B").is_err(), "a stale writer can't overwrite newer saves");
        rename(&mut a, &mut doc_a, "From A again").unwrap();
        assert_eq!(Store::open(&path).unwrap().1.name, "From A again");
    }
}
