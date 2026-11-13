//! The Spark server: the `/ui` WebSocket channel, the `/mcp` endpoint, document assets and the
//! built web UI, all on 127.0.0.1 behind a per-user bearer token.

pub mod commands;
pub mod config;
pub mod connect;
pub mod edit;
pub mod export;
pub mod fonts;
pub mod generate;
pub mod hub;
pub mod images;
pub mod layout;
pub mod mcp;
pub mod providers;
pub mod relay;
pub mod runner;
pub mod secrets;
pub mod settings;
mod ui;
pub mod views;

use std::{net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};

use axum::{
    Router,
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, Request, State, WebSocketUpgrade},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
};
use tower_http::services::{ServeDir, ServeFile};

pub use config::Config;

/// The home directory agent configs live in (`SPARK_HOME_DIR` overrides it for tests).
pub fn home_dir() -> PathBuf {
    std::env::var_os("SPARK_HOME_DIR").map(PathBuf::from).or_else(dirs::home_dir).unwrap_or_else(std::env::temp_dir)
}
pub use hub::Hub;

#[derive(Clone)]
pub struct AppState {
    pub hub: Arc<Hub>,
    pub port: u16,
}

pub struct ServerOptions {
    pub port: u16,
    pub web_dir: Option<PathBuf>,
    pub config: Config,
}

impl AppState {
    fn origin_ok(&self, origin: &str) -> bool {
        let p = self.port;
        origin == format!("http://127.0.0.1:{p}")
            || origin == format!("http://localhost:{p}")
            || std::env::var("SPARK_ALLOW_ORIGIN").is_ok_and(|o| o == origin)
    }

    fn host_ok(&self, host: &str) -> bool {
        let p = self.port;
        [format!("127.0.0.1:{p}"), format!("localhost:{p}"), format!("[::1]:{p}")].iter().any(|h| h == host)
    }

    pub fn token_ok(&self, headers: &HeaderMap, query: Option<&str>) -> bool {
        let bearer = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .or(query);
        bearer.is_some_and(|t| constant_eq(t.as_bytes(), self.hub.config.token.as_bytes()))
    }
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// DNS-rebinding and cross-site guard: the Host must be loopback on our port, and an Origin, when
/// present, must be the Spark UI itself.
async fn guard(State(app): State<AppState>, req: Request, next: Next) -> Response {
    let headers = req.headers();
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok()).unwrap_or("");
    if !app.host_ok(host) {
        return (StatusCode::FORBIDDEN, "forbidden host").into_response();
    }
    if let Some(origin) = headers.get(header::ORIGIN)
        && !app.origin_ok(origin.to_str().unwrap_or(""))
    {
        return (StatusCode::FORBIDDEN, "forbidden origin").into_response();
    }
    // Font requests carry no token and can make Spark download, so only Spark's own pages may send
    // them. Browsers label every request with where it came from; other clients send no label.
    if req.uri().path().starts_with("/fonts/")
        && headers.get("sec-fetch-site").is_some_and(|v| !matches!(v.as_bytes(), b"same-origin" | b"none"))
    {
        return (StatusCode::FORBIDDEN, "forbidden site").into_response();
    }
    let mut res = next.run(req).await;
    // Only Spark's own script files run in its pages, so markup inside a design (an SVG event
    // attribute, say) can never act as the UI.
    res.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("script-src 'self'; object-src 'none'; base-uri 'none'"),
    );
    // An updated app serves a new UI on the same port; without this the desktop webview keeps
    // running its cached copy of the old one.
    res.headers_mut().entry(header::CACHE_CONTROL).or_insert(HeaderValue::from_static("no-cache"));
    res
}

#[derive(serde::Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

async fn ui_socket(
    State(app): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<TokenQuery>,
    ws: WebSocketUpgrade,
) -> Response {
    if !app.token_ok(&headers, q.token.as_deref()) {
        return (StatusCode::UNAUTHORIZED, "missing or wrong token").into_response();
    }
    let hub = app.hub.clone();
    ws.max_message_size(256 << 20).on_upgrade(move |socket| ui::client(socket, hub))
}

async fn asset(State(app): State<AppState>, Path(hash): Path<String>) -> Response {
    for doc in app.hub.open_docs() {
        if let Some(a) = doc.lock().unwrap().editor.doc.assets.get(&hash) {
            return (
                [
                    (header::CONTENT_TYPE, a.mime.clone()),
                    (header::CACHE_CONTROL, "public, max-age=31536000, immutable".into()),
                ],
                a.bytes.as_ref().clone(),
            )
                .into_response();
        }
    }
    StatusCode::NOT_FOUND.into_response()
}

/// Stores an uploaded image in a document's assets and returns its hash and pixel size.
async fn upload_asset(
    State(app): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<std::collections::HashMap<String, String>>,
    body: Bytes,
) -> Response {
    if !app.token_ok(&headers, None) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let Some(doc) = q.get("file").and_then(|f| app.hub.get(f)) else {
        return (StatusCode::NOT_FOUND, "file not open").into_response();
    };
    match add_asset(&mut doc.lock().unwrap(), &body) {
        Ok(v) => axum::Json(v).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, e).into_response(),
    }
}

/// Adds image bytes to a document as a content-addressed asset.
pub fn add_asset(open: &mut hub::OpenDoc, bytes: &[u8]) -> Result<serde_json::Value, String> {
    let mime =
        spark_core::asset::image_mime(bytes).ok_or("not a supported image (png, jpeg, gif, webp, avif or svg)")?;
    let hash = spark_core::asset::asset_hash(bytes);
    let (width, height) = imagesize::blob_size(bytes).map(|s| (s.width as u64, s.height as u64)).unwrap_or((512, 512));
    let op = spark_core::Op::AddAsset { hash: hash.clone(), mime: mime.into(), bytes: Arc::new(bytes.to_vec()) };
    let ops = open.editor.apply_unrecorded(vec![op]).map_err(|e| e.to_string())?;
    open.save(&ops);
    Ok(serde_json::json!({"hash": hash, "mime": mime, "width": width, "height": height}))
}

/// Uploads a `.spark` file (browser drag and drop) into the files dir and opens it.
async fn import_file(
    State(app): State<AppState>,
    headers: HeaderMap,
    Query(q): Query<std::collections::HashMap<String, String>>,
    body: Bytes,
) -> Response {
    if !app.token_ok(&headers, None) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let name = q.get("name").map(|n| n.trim_end_matches(".spark").to_string()).unwrap_or_else(|| "Imported".into());
    let path = app.hub.config.new_file_path(&name);
    let result = std::fs::create_dir_all(&app.hub.config.files_dir)
        .and_then(|_| std::fs::write(&path, &body))
        .map_err(|e| e.to_string())
        .and_then(|_| app.hub.open_path(&path));
    match result {
        Ok(doc) => axum::Json(doc.lock().unwrap().file_info()).into_response(),
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            (StatusCode::BAD_REQUEST, e).into_response()
        }
    }
}

pub fn router(state: AppState, web_dir: Option<PathBuf>) -> Router {
    let mut app = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/ui", any(ui_socket))
        .route("/assets/{hash}", get(asset))
        .route("/fonts/css", get(fonts::css))
        .route("/fonts/list", get(fonts::list))
        .route("/fonts/file/{*path}", get(fonts::file))
        .route("/api/import", post(import_file))
        .route("/api/assets", post(upload_asset))
        .route("/mcp", post(mcp::post).get(mcp::get).delete(mcp::delete))
        .route("/mcp/config.json", get(mcp::config))
        .route("/.well-known/{*rest}", any(|| async { StatusCode::NOT_FOUND }));
    if let Some(dir) = web_dir {
        let index = dir.join("index.html");
        app = app.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)));
    }
    // Uploads, imports and base64 images in MCP calls can be large; matches the /ui message limit.
    app.layer(DefaultBodyLimit::max(256 << 20))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}

/// Binds 127.0.0.1 on `port`, or the next free port after it (port 0 picks any free port).
pub async fn bind(port: u16) -> std::io::Result<tokio::net::TcpListener> {
    if port == 0 {
        return tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], 0))).await;
    }
    let mut last = None;
    for p in port..port.saturating_add(20) {
        match tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], p))).await {
            Ok(l) => return Ok(l),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap())
}

pub async fn serve(opts: ServerOptions) -> anyhow::Result<()> {
    let listener = bind(opts.port).await?;
    let port = listener.local_addr()?.port();
    println!("Spark is running at http://127.0.0.1:{port}/#token={}", opts.config.token);
    serve_on(listener, opts.web_dir, opts.config).await
}

pub async fn serve_on(
    listener: tokio::net::TcpListener,
    web_dir: Option<PathBuf>,
    config: Config,
) -> anyhow::Result<()> {
    let (_, server) = prepare(listener, web_dir, config)?;
    server.await
}

/// Sets up the server on `listener` and returns its hub (for the desktop shell to drive) and the
/// future that serves requests. Background autosave and agent housekeeping start when it's polled.
pub fn prepare(
    listener: tokio::net::TcpListener,
    web_dir: Option<PathBuf>,
    config: Config,
) -> anyhow::Result<(Arc<Hub>, impl std::future::Future<Output = anyhow::Result<()>>)> {
    let port = listener.local_addr()?.port();
    config.write_port(port)?;
    export::preload_fonts();
    let hub = Hub::new(config);
    hub.port.store(port, std::sync::atomic::Ordering::Relaxed);
    let h = hub.clone();
    let server = async move {
        let flusher = h.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
                flusher.flush();
                flusher.prune_agents();
            }
        });
        axum::serve(listener, router(AppState { hub: h, port }, web_dir)).await?;
        Ok(())
    };
    Ok((hub, server))
}
