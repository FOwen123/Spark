//! The shared registry of open documents and connected UI clients.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use serde_json::{Value, json};
use spark_core::{Document, Node, NodeId, NodeKind, Op, Parent, history::Editor, id::is_file_id, store::Store};
use tokio::sync::{mpsc::UnboundedSender, oneshot};

use crate::{config::Config, views};

pub type ClientId = u64;
pub type Tx = UnboundedSender<String>;

pub struct OpenDoc {
    pub editor: Editor,
    pub store: Store,
    pub path: PathBuf,
    /// UI clients viewing this document.
    pub subscribers: HashMap<ClientId, Tx>,
    /// The drag or resize in progress, if any.
    pub session: Option<crate::edit::Session>,
    /// When the document last changed (for checkpointing once edits pause).
    pub last_write: Instant,
    /// Why the last save failed; edits stay in memory and saving retries on the next change.
    pub save_error: Option<String>,
}

pub type DocRef = Arc<Mutex<OpenDoc>>;

impl OpenDoc {
    pub fn id(&self) -> &str {
        &self.editor.doc.id
    }

    /// Persists pending changes. Errors are logged; the dirty set is kept for the next try.
    pub fn save(&mut self, ops: &[Op]) {
        if !ops.is_empty() {
            self.last_write = Instant::now();
        }
        match self.store.save(&mut self.editor.doc, ops) {
            Err(e) => {
                eprintln!("spark: saving {} failed: {e}", self.path.display());
                self.save_error = Some(e.to_string());
            }
            Ok(()) if self.save_error.is_some() => self.save_error = None,
            Ok(()) => return,
        }
        let msg = json!({"t": "saveError", "file": self.id(), "message": self.save_error});
        self.broadcast(&msg);
    }

    pub fn broadcast(&mut self, msg: &Value) {
        let text = msg.to_string();
        self.subscribers.retain(|_, tx| tx.send(text.clone()).is_ok());
    }

    /// Saves, then sends the patch for `ops` to every subscriber.
    pub fn commit_ops(&mut self, ops: &[Op]) {
        self.save(ops);
        let patch = views::patch(self, ops);
        self.broadcast(&patch);
    }

    pub fn file_info(&self) -> Value {
        json!({"id": self.editor.doc.id, "name": self.editor.doc.name, "path": self.path})
    }
}

/// An MCP client session, shown in the UI as an agent.
pub struct Agent {
    pub name: String,
    pub transport: String,
    /// Artboards the agent is working on, per file, with when it last touched them.
    pub working: HashMap<String, HashMap<NodeId, Instant>>,
    pub last_seen: Instant,
}

type RenderReply = oneshot::Sender<Result<Vec<u8>, String>>;

/// A native action provided by the desktop shell, called with a command name.
pub type NativeAction = Box<dyn Fn(&str) + Send + Sync>;

pub struct Hub {
    pub config: Config,
    docs: Mutex<HashMap<String, DocRef>>,
    /// Held while opening a path, so one file never ends up open as two documents.
    opening: Mutex<()>,
    clients: Mutex<HashMap<ClientId, Tx>>,
    next_client: AtomicU64,
    /// The file each UI client is looking at, most recent last.
    viewing: Mutex<Vec<(ClientId, String)>>,
    pub agents: Mutex<HashMap<String, Agent>>,
    renders: Mutex<HashMap<u64, RenderReply>>,
    next_call: AtomicU64,
    /// The port the server listens on (for absolute asset URLs).
    pub port: std::sync::atomic::AtomicU16,
    /// Native actions the desktop shell provides (e.g. "openFile" shows the system dialog).
    pub native: std::sync::OnceLock<NativeAction>,
}

/// Working indicators fade after this long without activity.
pub const WORK_TTL: Duration = Duration::from_secs(60);
/// Agents that haven't called a tool for this long drop off the list.
pub const AGENT_TTL: Duration = Duration::from_secs(600);

/// Extracts a file id from a ULID, a `/file/<id>/<pageId>` route or a `spark://file/<id>/...` link.
pub fn file_id_from_ref(r: &str) -> Option<String> {
    let r = r.trim();
    if is_file_id(r) {
        return Some(r.to_string());
    }
    let rest = r.strip_prefix("spark://").map(|s| format!("/{s}")).unwrap_or_else(|| r.to_string());
    let rest = rest.split(['?', '#']).next()?;
    let mut parts = rest.trim_start_matches('/').split('/');
    (parts.next()? == "file").then_some(())?;
    parts.next().filter(|id| is_file_id(id)).map(str::to_string)
}

/// The page id inside a `/file/<id>/<pageId>` route or link, if any.
pub fn page_from_ref(r: &str) -> Option<String> {
    let r = r.split(['?', '#']).next()?;
    let mut parts = r.trim_start_matches("spark://").trim_start_matches('/').split('/');
    (parts.next()? == "file").then_some(())?;
    parts.next()?;
    parts.next().filter(|p| p.starts_with("p-")).map(str::to_string)
}

impl Hub {
    pub fn new(config: Config) -> Arc<Hub> {
        Arc::new(Hub {
            config,
            docs: Mutex::default(),
            opening: Mutex::default(),
            clients: Mutex::default(),
            next_client: AtomicU64::new(1),
            viewing: Mutex::default(),
            agents: Mutex::default(),
            renders: Mutex::default(),
            next_call: AtomicU64::new(1),
            port: Default::default(),
            native: std::sync::OnceLock::new(),
        })
    }

    pub fn add_client(&self, tx: Tx) -> ClientId {
        let id = self.next_client.fetch_add(1, Ordering::Relaxed);
        self.clients.lock().unwrap().insert(id, tx);
        id
    }

    pub fn remove_client(&self, id: ClientId) {
        self.clients.lock().unwrap().remove(&id);
        self.viewing.lock().unwrap().retain(|(c, _)| *c != id);
        for doc in self.open_docs() {
            doc.lock().unwrap().subscribers.remove(&id);
        }
    }

    pub fn client_ids(&self) -> Vec<ClientId> {
        self.clients.lock().unwrap().keys().copied().collect()
    }

    pub fn send_to(&self, client: ClientId, msg: &Value) -> bool {
        self.clients.lock().unwrap().get(&client).is_some_and(|tx| tx.send(msg.to_string()).is_ok())
    }

    pub fn broadcast_all(&self, msg: &Value) {
        let text = msg.to_string();
        self.clients.lock().unwrap().retain(|_, tx| tx.send(text.clone()).is_ok());
    }

    pub fn open_docs(&self) -> Vec<DocRef> {
        self.docs.lock().unwrap().values().cloned().collect()
    }

    pub fn get(&self, id: &str) -> Option<DocRef> {
        self.docs.lock().unwrap().get(id).cloned()
    }

    /// Archives a file from the recent list (see `Config::archive`).
    pub fn archive(&self, id: &str) -> Result<PathBuf, String> {
        let path = self.release(id)?;
        self.config.archive(id, &path)
    }

    /// Moves a file from the recent list to the Trash (see `Config::trash`).
    pub fn trash(&self, id: &str) -> Result<(), String> {
        let path = self.release(id)?;
        self.config.trash(id, &path)
    }

    /// Lets go of a file so it can be moved: refuses while a tab has it open or it holds edits Spark
    /// couldn't save; otherwise unloads it so its store is closed. Returns its path.
    fn release(&self, id: &str) -> Result<PathBuf, String> {
        let path = {
            let mut docs = self.docs.lock().unwrap();
            if let Some(d) = docs.get(id) {
                {
                    let o = d.lock().unwrap();
                    if !o.subscribers.is_empty() {
                        return Err(format!("{} is open in a tab. Close it first.", o.editor.doc.name));
                    }
                    if o.save_error.is_some() {
                        return Err(format!(
                            "Spark couldn't save the latest edits to {}. Open it and fix that first.",
                            o.editor.doc.name
                        ));
                    }
                }
                if Arc::strong_count(d) > 1 {
                    return Err("An agent is using this file right now. Try again in a moment.".into());
                }
                let d = docs.remove(id).unwrap();
                let path = d.lock().unwrap().path.clone();
                drop(d); // Closes the store, which flushes the WAL into the file.
                path
            } else {
                self.config.recent().into_iter().find(|f| f.id == id).ok_or("This file isn't in the recent list.")?.path
            }
        };
        Ok(path)
    }

    fn register(&self, store: Store, doc: Document, path: &Path) -> DocRef {
        let id = doc.id.clone();
        let mut docs = self.docs.lock().unwrap();
        if let Some(existing) = docs.get(&id) {
            return existing.clone();
        }
        self.config.remember(&id, &doc.name, path);
        let open = Arc::new(Mutex::new(OpenDoc {
            editor: Editor::new(doc),
            store,
            path: path.to_path_buf(),
            subscribers: HashMap::new(),
            session: None,
            last_write: Instant::now(),
            save_error: None,
        }));
        docs.insert(id, open.clone());
        open
    }

    pub fn open_path(&self, path: &Path) -> Result<DocRef, String> {
        let path = path.canonicalize().map_err(|e| format!("can't open {}: {e}", path.display()))?;
        let _opening = self.opening.lock().unwrap();
        if let Some(d) = self.open_docs().into_iter().find(|d| d.lock().unwrap().path == path) {
            return Ok(d);
        }
        let (mut store, mut doc) = Store::open(&path).map_err(|e| format!("can't open {}: {e}", path.display()))?;
        if self.get(&doc.id).is_some() {
            // A copy of an open file (duplicated on disk) gets its own id, so edits reach the copy.
            doc.id = spark_core::id::new_file_id();
            doc.dirty.meta = true;
            store.save(&mut doc, &[]).map_err(|e| format!("can't open {}: {e}", path.display()))?;
        }
        Ok(self.register(store, doc, &path))
    }

    /// Opens a file by id (from the recent list), route, `spark://` link or `.spark` path.
    pub fn open_ref(&self, r: &str) -> Result<DocRef, String> {
        if let Some(id) = file_id_from_ref(r) {
            if let Some(d) = self.get(&id) {
                return Ok(d);
            }
            let recent = self.config.recent().into_iter().find(|f| f.id == id);
            return match recent {
                Some(f) => self.open_path(&f.path),
                None => Err(format!("file {id} is not open and not in the recent files list")),
            };
        }
        let path = PathBuf::from(r);
        if path.extension().is_some_and(|e| e == "spark") || path.exists() {
            return self.open_path(&path);
        }
        Err(format!("`{r}` is not a file id, route, spark:// link or .spark path"))
    }

    /// Creates a new file in the files dir with one empty artboard.
    pub fn create(&self, name: &str) -> Result<DocRef, String> {
        let name = if name.trim().is_empty() { "Untitled" } else { name.trim() };
        let mut doc = Document::new(name);
        let page = doc.pages[0].id;
        let mut board = Node::new(doc.alloc_node(), NodeKind::Frame);
        board.name = "Desktop".into();
        board.style = serde_json::from_value(json!({
            "position": "absolute", "left": "0px", "top": "0px", "width": "1440px", "height": "1024px",
            "overflow": "clip", "fills": [{"type": "solid", "color": "#ffffff"}],
        }))
        .unwrap();
        let key = doc.key_at(Parent::Page(page), None, None);
        doc.apply(Op::Insert { node: board, parent: Parent::Page(page), key }).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&self.config.files_dir).map_err(|e| e.to_string())?;
        let path = self.config.new_file_path(name);
        let store = Store::create(&path, &mut doc).map_err(|e| e.to_string())?;
        let path = path.canonicalize().map_err(|e| e.to_string())?;
        Ok(self.register(store, doc, &path))
    }

    /// Records which file a UI client is looking at.
    pub fn set_viewing(&self, client: ClientId, file: Option<&str>) {
        let mut v = self.viewing.lock().unwrap();
        v.retain(|(c, _)| *c != client);
        if let Some(f) = file {
            v.push((client, f.to_string()));
        }
    }

    /// The file the user is looking at: the most recently viewed one, or the only open file.
    pub fn viewed(&self) -> Option<DocRef> {
        let last = self.viewing.lock().unwrap().last().map(|(_, f)| f.clone());
        if let Some(d) = last.and_then(|f| self.get(&f)) {
            return Some(d);
        }
        let docs = self.open_docs();
        if docs.len() == 1 { docs.into_iter().next() } else { None }
    }

    pub fn is_viewed(&self, file: &str) -> bool {
        self.viewing.lock().unwrap().iter().any(|(_, f)| f == file)
    }

    // ---- agents ------------------------------------------------------------------------------

    pub fn agents_json(&self) -> Value {
        let agents = self.agents.lock().unwrap();
        let mut list: Vec<Value> = agents
            .iter()
            .map(|(id, a)| {
                let files: serde_json::Map<String, Value> =
                    a.working.iter().map(|(f, boards)| (f.clone(), json!(boards.keys().collect::<Vec<_>>()))).collect();
                json!({"id": id, "name": a.name, "transport": a.transport, "files": files})
            })
            .collect();
        list.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        json!({"t": "agents", "agents": list})
    }

    pub fn broadcast_agents(&self) {
        let msg = self.agents_json();
        self.broadcast_all(&msg);
    }

    /// Notes that an agent made a call (creating it on first sight).
    pub fn agent_seen(&self, session: &str, name: &str, transport: &str) {
        let mut agents = self.agents.lock().unwrap();
        let new = !agents.contains_key(session);
        let a = agents.entry(session.to_string()).or_insert_with(|| Agent {
            name: name.to_string(),
            transport: transport.to_string(),
            working: HashMap::new(),
            last_seen: Instant::now(),
        });
        let renamed = a.name != name && !name.is_empty();
        if renamed {
            a.name = name.to_string();
        }
        a.last_seen = Instant::now();
        drop(agents);
        if new || renamed {
            self.broadcast_agents();
        }
    }

    /// Marks artboards an agent is working on and sweeps the touched nodes.
    pub fn agent_activity(&self, session: &str, file: &str, boards: &[NodeId], nodes: &[NodeId], write: bool) {
        let changed = {
            let mut agents = self.agents.lock().unwrap();
            match agents.get_mut(session) {
                Some(a) if write && !boards.is_empty() => {
                    let w = a.working.entry(file.to_string()).or_default();
                    let before = w.len();
                    for b in boards {
                        w.insert(*b, Instant::now());
                    }
                    w.len() != before
                }
                _ => false,
            }
        };
        if changed {
            self.broadcast_agents();
        }
        if !nodes.is_empty() {
            self.broadcast_all(&json!({"t": "activity", "file": file, "ids": nodes, "write": write, "agent": session}));
        }
    }

    /// Clears an agent's working indicators in a file (all, or just the given artboards).
    pub fn agent_finish(&self, session: &str, file: &str, boards: Option<&[NodeId]>) {
        {
            let mut agents = self.agents.lock().unwrap();
            let Some(a) = agents.get_mut(session) else { return };
            match boards {
                None => {
                    a.working.remove(file);
                }
                Some(list) => {
                    if let Some(w) = a.working.get_mut(file) {
                        w.retain(|b, _| !list.contains(b));
                        if w.is_empty() {
                            a.working.remove(file);
                        }
                    }
                }
            }
        }
        self.broadcast_agents();
    }

    pub fn remove_agent(&self, session: &str) {
        if self.agents.lock().unwrap().remove(session).is_some() {
            self.broadcast_agents();
        }
    }

    /// Expires stale working indicators and idle agents.
    pub fn prune_agents(&self) {
        let changed = {
            let mut agents = self.agents.lock().unwrap();
            let before: usize =
                agents.values().map(|a| a.working.values().map(HashMap::len).sum::<usize>()).sum::<usize>()
                    + agents.len();
            agents.retain(|_, a| a.last_seen.elapsed() < AGENT_TTL);
            for a in agents.values_mut() {
                for w in a.working.values_mut() {
                    w.retain(|_, t| t.elapsed() < WORK_TTL);
                }
                a.working.retain(|_, w| !w.is_empty());
            }
            let after: usize =
                agents.values().map(|a| a.working.values().map(HashMap::len).sum::<usize>()).sum::<usize>()
                    + agents.len();
            before != after
        };
        if changed {
            self.broadcast_agents();
        }
    }

    // ---- rendering ---------------------------------------------------------------------------

    /// Asks a connected UI to rasterize a node to PNG. Prefers a window viewing the file.
    pub async fn render(&self, file: &str, node: NodeId, scale: f64) -> Result<Vec<u8>, String> {
        let viewer = self.viewing.lock().unwrap().iter().rev().find(|(_, f)| f == file).map(|(c, _)| *c);
        let subscriber = self.get(file).and_then(|d| d.lock().unwrap().subscribers.keys().next().copied());
        let any = self.client_ids().into_iter().next();
        let client = viewer.or(subscriber).or(any).ok_or_else(|| {
            "Spark's window isn't open, so it can't render. Ask the user to open Spark, then try again.".to_string()
        })?;
        let call = self.next_call.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.renders.lock().unwrap().insert(call, tx);
        let msg = json!({"t": "render", "callId": call, "file": file, "node": node, "scale": scale});
        if !self.send_to(client, &msg) {
            self.renders.lock().unwrap().remove(&call);
            return Err("lost the connection to Spark's window".into());
        }
        match tokio::time::timeout(Duration::from_secs(20), rx).await {
            Ok(Ok(r)) => r,
            _ => {
                self.renders.lock().unwrap().remove(&call);
                Err("rendering timed out; is the Spark window open and visible?".into())
            }
        }
    }

    pub fn render_done(&self, call: u64, result: Result<Vec<u8>, String>) {
        if let Some(tx) = self.renders.lock().unwrap().remove(&call) {
            let _ = tx.send(result);
        }
    }

    /// Opens a file (id, route, spark:// link or path) and asks the UI to show it, optionally on a
    /// page and with a node selected. Used for deep links, opened files and the File menu.
    pub fn reveal(&self, r: &str) -> Result<String, String> {
        let r = r.trim();
        let file_url = reqwest::Url::parse(r).ok().filter(|u| u.scheme() == "file");
        let doc = match file_url.map(|u| u.to_file_path()) {
            Some(Ok(path)) => self.open_path(&path)?,
            Some(Err(())) => return Err(format!("`{r}` is not a local file")),
            None => self.open_ref(r)?,
        };
        let node = r.split_once("node=").map(|(_, n)| n.split('&').next().unwrap_or("").to_string());
        let id = {
            let mut d = doc.lock().unwrap();
            // Show the linked page and select the linked node before the UI asks for a snapshot.
            if let Some(p) = page_from_ref(r)
                .and_then(|p| p.parse().ok())
                .filter(|p| d.editor.doc.page(*p).is_some_and(|pg| !pg.divider))
            {
                d.editor.page = Some(p);
            }
            if let Some(n) = node.as_deref().and_then(|n| n.parse::<NodeId>().ok()).filter(|n| d.editor.doc.is_live(*n))
            {
                d.editor.page = d.editor.doc.page_of(n).or(d.editor.page);
                d.editor.selection = vec![n];
            }
            d.id().to_string()
        };
        self.broadcast_all(
            &json!({"t": "openTab", "file": id, "activate": true, "page": page_from_ref(r), "node": node}),
        );
        Ok(id)
    }

    /// Forwards a native menu command to the UI.
    pub fn menu(&self, cmd: &str) {
        self.broadcast_all(&json!({"t": "menu", "cmd": cmd}));
    }

    /// Saves every document with pending bookkeeping (measured layout), and folds the write-ahead
    /// log into each file once edits pause, so the `.spark` file alone is always complete.
    pub fn flush(&self) {
        for d in self.open_docs() {
            let mut d = d.lock().unwrap();
            d.save(&[]);
            if d.store.has_unsynced()
                && d.last_write.elapsed() > Duration::from_secs(2)
                && let Err(e) = d.store.checkpoint()
            {
                eprintln!("spark: checkpoint of {} failed: {e}", d.path.display());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_file_refs() {
        let id = spark_core::id::new_file_id();
        assert_eq!(file_id_from_ref(&id).as_deref(), Some(id.as_str()));
        assert_eq!(file_id_from_ref(&format!("/file/{id}/p-1")).as_deref(), Some(id.as_str()));
        assert_eq!(file_id_from_ref(&format!("spark://file/{id}/p-1?node=1-0")).as_deref(), Some(id.as_str()));
        assert_eq!(page_from_ref(&format!("spark://file/{id}/p-2?node=1-0")).as_deref(), Some("p-2"));
        assert_eq!(file_id_from_ref("/tmp/x.spark"), None);
        assert_eq!(file_id_from_ref("file/nope"), None);
    }

    #[test]
    fn concurrent_opens_share_one_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Shared.spark");
        Store::create(&path, &mut Document::new("Shared")).unwrap();
        let hub = Hub::new(Config::load(dir.path().join("c"), dir.path().join("f")).unwrap());
        let opened: Vec<DocRef> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..8).map(|_| s.spawn(|| hub.open_path(&path).unwrap())).collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        assert!(opened.iter().all(|d| Arc::ptr_eq(d, &opened[0])), "one file, one document");
    }

    #[test]
    fn reveals_percent_encoded_file_urls() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Review #1 é.spark");
        Store::create(&path, &mut Document::new("Review")).unwrap();
        let hub = Hub::new(Config::load(dir.path().join("c"), dir.path().join("f")).unwrap());
        let url = reqwest::Url::from_file_path(&path).unwrap();
        assert!(url.as_str().contains("%23"));
        hub.reveal(url.as_str()).unwrap();
    }

    #[test]
    fn creates_and_reopens_files() {
        let dir = tempfile::tempdir().unwrap();
        let hub = Hub::new(Config::load(dir.path().join("c"), dir.path().join("f")).unwrap());
        let doc = hub.create("Hello").unwrap();
        let (id, path) = {
            let d = doc.lock().unwrap();
            (d.id().to_string(), d.path.clone())
        };
        assert!(path.ends_with("Hello.spark"));
        assert_eq!(hub.config.recent()[0].id, id);
        let again = hub.open_ref(&id).unwrap();
        assert!(Arc::ptr_eq(&doc, &again));
        let hub2 = Hub::new(Config::load(dir.path().join("c"), dir.path().join("f")).unwrap());
        let reopened = hub2.open_ref(&id).unwrap();
        assert_eq!(reopened.lock().unwrap().editor.doc.node_count(), 1);
        assert!(hub2.open_ref("nonsense").is_err());
    }
}
