//! Image requests and generation. `spark-gen://agent?…` creates a pending request that any agent
//! can fulfil; `spark-gen://<provider>/<model>?…` queues a generation job for an enabled provider.

use std::{collections::HashMap, sync::Arc, sync::Mutex};

use serde_json::{Value, json};
use spark_core::{ImageStatus, html::ImageSrc};

use crate::{hub::Hub, images, settings::Settings};

/// A parsed `spark-gen://` URL.
#[derive(Debug, Clone, PartialEq)]
pub struct GenUrl {
    pub provider: String,
    pub model: Option<String>,
    pub prompt: String,
    pub aspect_ratio: String,
    pub reference_nodes: Vec<String>,
}

fn decode(s: &str) -> String {
    let s = s.replace('+', " ");
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && b[i + 1].is_ascii_hexdigit()
            && b[i + 2].is_ascii_hexdigit()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn parse_gen(url: &str) -> Result<GenUrl, String> {
    let rest = url.strip_prefix("spark-gen://").ok_or("not a spark-gen URL")?;
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let mut parts = path.trim_matches('/').splitn(2, '/');
    let provider =
        parts.next().filter(|p| !p.is_empty()).ok_or("spark-gen URL needs a provider (agent, openai, …)")?.to_string();
    let model = parts.next().filter(|m| !m.is_empty()).map(str::to_string);
    let mut g =
        GenUrl { provider, model, prompt: String::new(), aspect_ratio: "1:1".into(), reference_nodes: Vec::new() };
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        match k {
            "prompt" => g.prompt = decode(v),
            "aspect_ratio" => g.aspect_ratio = decode(v),
            "reference_nodes" => {
                g.reference_nodes =
                    decode(v).split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
            }
            _ => {}
        }
    }
    if g.prompt.trim().is_empty() {
        return Err("spark-gen URL needs a prompt".into());
    }
    let (w, h) =
        ratio(&g.aspect_ratio).ok_or_else(|| format!("bad aspect_ratio `{}` (use W:H, e.g. 16:9)", g.aspect_ratio))?;
    if w <= 0.0 || h <= 0.0 {
        return Err("aspect_ratio must be positive".into());
    }
    Ok(g)
}

pub fn ratio(r: &str) -> Option<(f64, f64)> {
    let (a, b) = r.split_once(':')?;
    Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
}

fn request_id() -> String {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).expect("os random");
    format!("img-{}", b.iter().map(|x| format!("{x:02x}")).collect::<String>())
}

/// Generation jobs collected while importing, started after the document is saved.
#[derive(Default)]
pub struct JobQueue(pub Mutex<Vec<(String, GenUrl)>>);

/// The image state stored on a placeholder node, with a default size from the aspect ratio.
fn job_json(g: &GenUrl, id: &str, status: ImageStatus) -> Value {
    let (w, h) = ratio(&g.aspect_ratio).unwrap_or((1.0, 1.0));
    let width = 512.0;
    json!({
        "id": id, "status": status.as_str(), "provider": g.provider, "model": g.model, "prompt": g.prompt,
        "aspectRatio": g.aspect_ratio, "referenceNodes": g.reference_nodes,
        "width": width, "height": (width * h / w).round(),
    })
}

/// A resolver for the HTML importer: prefetched images, agent requests and provider jobs.
pub fn resolver<'a>(
    loaded: &'a HashMap<String, images::Loaded>,
    jobs: &'a JobQueue,
    settings: &'a Settings,
) -> impl FnMut(&str) -> ImageSrc + 'a {
    move |url: &str| {
        if url.starts_with("spark-gen://") {
            let g = match parse_gen(url) {
                Ok(g) => g,
                Err(e) => return ImageSrc::Failed(e),
            };
            let id = request_id();
            if g.provider == "agent" {
                return ImageSrc::Job(job_json(&g, &id, ImageStatus::Pending));
            }
            if let Err(e) = crate::generate::check_enabled(settings, &g.provider) {
                return ImageSrc::Failed(e);
            }
            jobs.0.lock().unwrap().push((id.clone(), g.clone()));
            return ImageSrc::Job(job_json(&g, &id, ImageStatus::Processing));
        }
        match loaded.get(url) {
            Some(l) => images::to_src(url, l),
            None => ImageSrc::Failed(format!("unsupported image URL {}", url.chars().take(64).collect::<String>())),
        }
    }
}

/// Starts queued generation jobs for a file in the background.
pub fn start(hub: Arc<Hub>, file: String, jobs: JobQueue) {
    let jobs = jobs.0.into_inner().unwrap();
    if jobs.is_empty() {
        return;
    }
    tokio::spawn(crate::generate::run(hub, file, jobs));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_spark_gen_urls() {
        let g = parse_gen("spark-gen://agent?prompt=A%20red+kite&aspect_ratio=16:9&reference_nodes=1-0,2-0").unwrap();
        assert_eq!(g.provider, "agent");
        assert_eq!(g.prompt, "A red kite");
        assert_eq!(g.reference_nodes, vec!["1-0", "2-0"]);
        let g = parse_gen("spark-gen://openai/gpt-image-1?prompt=x").unwrap();
        assert_eq!((g.provider.as_str(), g.model.as_deref()), ("openai", Some("gpt-image-1")));
        assert!(parse_gen("spark-gen://agent?aspect_ratio=1:1").is_err());
        assert!(parse_gen("spark-gen://agent?prompt=x&aspect_ratio=wide").is_err());
    }
}
