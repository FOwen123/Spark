//! Fonts beyond the installed ones: designs can name any Google Fonts family. The first time the
//! canvas needs one, Spark downloads it and keeps a copy in its config dir, so it works offline
//! after that. The UI loads `/fonts/css?family=…`, whose font files point back at `/fonts/file/…`.

use std::{collections::HashMap, sync::OnceLock, time::Duration};

use axum::{
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};

use crate::AppState;

/// Google picks the font format by browser; this one gets WOFF2 split into Unicode ranges, so a
/// Chinese font downloads only the characters a design uses.
const BROWSER_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";
const MAX_FONT: usize = 25 << 20;

/// Google's metadata, CSS and font file hosts. `SPARK_GOOGLE_FONTS_URL` points all three at one
/// server for tests.
fn hosts() -> [String; 3] {
    match std::env::var("SPARK_GOOGLE_FONTS_URL") {
        Ok(base) => [base.clone(), base.clone(), base],
        Err(_) => {
            ["https://fonts.google.com", "https://fonts.googleapis.com", "https://fonts.gstatic.com"].map(String::from)
        }
    }
}

async fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(BROWSER_UA)
        .build()
        .map_err(|e| e.to_string())?;
    let res = client.get(url).send().await.map_err(|e| e.to_string())?;
    let bytes = res.error_for_status().map_err(|e| e.to_string())?.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > MAX_FONT {
        return Err(format!("{url} is larger than 25 MB"));
    }
    Ok(bytes.to_vec())
}

/// Installed font families (lowercased) and their weights and styles.
pub fn local_fonts() -> &'static HashMap<String, Vec<Value>> {
    static FONTS: OnceLock<HashMap<String, Vec<Value>>> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();
        let mut out: HashMap<String, Vec<Value>> = HashMap::new();
        for face in db.faces() {
            for (family, _) in &face.families {
                let style = match face.style {
                    fontdb::Style::Normal => "normal",
                    fontdb::Style::Italic => "italic",
                    fontdb::Style::Oblique => "oblique",
                };
                let entry =
                    json!({"weight": face.weight.0, "style": style, "isItalic": face.style != fontdb::Style::Normal});
                let list = out.entry(family.to_lowercase()).or_default();
                if !list.contains(&entry) {
                    list.push(entry);
                }
            }
        }
        out
    })
}

/// The Google Fonts catalog (family, style keys like `400i`, variable axes), cached for a week.
pub async fn google_fonts(app: &AppState) -> Option<Value> {
    let cache = app.hub.config.dir.join("google-fonts.json");
    let fresh = std::fs::metadata(&cache)
        .and_then(|m| m.modified())
        .is_ok_and(|t| t.elapsed().is_ok_and(|e| e.as_secs() < 7 * 86400));
    if fresh && let Some(v) = std::fs::read(&cache).ok().and_then(|b| serde_json::from_slice(&b).ok()) {
        return Some(v);
    }
    let [meta, ..] = hosts();
    let body = String::from_utf8(fetch(&format!("{meta}/metadata/fonts")).await.ok()?).ok()?;
    let v: Value = serde_json::from_str(body.trim_start_matches(")]}'").trim()).ok()?;
    let slim: Vec<Value> = v["familyMetadataList"]
        .as_array()?
        .iter()
        .map(|f| json!({"family": f["family"], "fonts": f["fonts"].as_object().map(|o| o.keys().cloned().collect::<Vec<_>>()), "axes": f["axes"]}))
        .collect();
    let slim = json!(slim);
    let _ = crate::config::write_private(&cache, slim.to_string().as_bytes());
    Some(slim)
}

/// The `family` parameter asking Google for every weight and style a catalog entry has, e.g.
/// `Fraunces:ital,wght@0,100..900;1,100..900` or `Poppins:wght@400;700`.
fn css_family(f: &Value) -> String {
    let family = f["family"].as_str().unwrap_or_default();
    let keys: Vec<&str> = f["fonts"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let italic = keys.iter().any(|k| k.ends_with('i'));
    let wght = f["axes"].as_array().into_iter().flatten().find(|a| a["tag"] == "wght");
    let tuples: Vec<String> = match wght {
        Some(a) => {
            let range = format!("{}..{}", a["min"].as_f64().unwrap_or(400.0), a["max"].as_f64().unwrap_or(400.0));
            if italic { vec![format!("0,{range}"), format!("1,{range}")] } else { vec![range] }
        }
        None => {
            let mut styles: Vec<(u8, u32)> = keys
                .iter()
                .filter_map(|k| Some((k.ends_with('i') as u8, k.trim_end_matches('i').parse().ok()?)))
                .collect();
            styles.sort();
            styles.dedup();
            styles.iter().map(|(i, w)| if italic { format!("{i},{w}") } else { w.to_string() }).collect()
        }
    };
    if tuples.is_empty() {
        return family.to_string();
    }
    format!("{family}:{}@{}", if italic { "ital,wght" } else { "wght" }, tuples.join(";"))
}

async fn google_css(app: &AppState, key: &str) -> Option<String> {
    let dir = app.hub.config.dir.join("fonts");
    let path = dir.join(format!("{}.css", key.replace(|c: char| !c.is_ascii_alphanumeric(), "-")));
    if let Ok(css) = std::fs::read_to_string(&path) {
        return Some(css);
    }
    let catalog = google_fonts(app).await?;
    let f = catalog.as_array()?.iter().find(|f| f["family"].as_str().is_some_and(|n| n.to_lowercase() == key))?;
    let [_, css_host, files] = hosts();
    let mut url = reqwest::Url::parse(&format!("{css_host}/css2")).ok()?;
    url.query_pairs_mut().append_pair("family", &css_family(f)).append_pair("display", "swap");
    let css = String::from_utf8(fetch(url.as_str()).await.ok()?).ok()?.replace(&format!("{files}/"), "/fonts/file/");
    let _ = crate::config::write_private(&path, css.as_bytes());
    Some(css)
}

/// `@font-face` rules for a Google Fonts family, or an empty stylesheet when the family is
/// installed, unknown, or can't be downloaded right now.
pub async fn css(State(app): State<AppState>, Query(q): Query<HashMap<String, String>>) -> Response {
    let Some(key) = q.get("family").map(|f| f.trim().to_lowercase()).filter(|k| !k.is_empty()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let body = if local_fonts().contains_key(&key) { None } else { google_css(&app, &key).await };
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], body.unwrap_or_default()).into_response()
}

/// A font file named in a `css` response, downloaded on first use.
pub async fn file(State(app): State<AppState>, Path(path): Path<String>) -> Response {
    let safe = path.chars().all(|c| c.is_ascii_alphanumeric() || "/-_.".contains(c))
        && path.split('/').all(|p| !p.is_empty() && !p.starts_with('.'));
    if !safe {
        return StatusCode::NOT_FOUND.into_response();
    }
    let local = app.hub.config.dir.join("fonts").join("files").join(&path);
    let bytes = match std::fs::read(&local) {
        Ok(b) => b,
        Err(_) => {
            let [.., files] = hosts();
            match fetch(&format!("{files}/{path}")).await {
                Ok(b) => {
                    let _ = crate::config::write_private(&local, &b);
                    b
                }
                Err(e) => return (StatusCode::BAD_GATEWAY, e).into_response(),
            }
        }
    };
    let mime = match path.rsplit('.').next() {
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        _ => "application/octet-stream",
    };
    ([(header::CONTENT_TYPE, mime), (header::CACHE_CONTROL, "public, max-age=31536000, immutable")], bytes)
        .into_response()
}
