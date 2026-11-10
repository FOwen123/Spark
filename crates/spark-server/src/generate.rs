//! Built-in image generation, bring your own key. Providers are adapters behind one trait; jobs
//! run asynchronously and fill their placeholder node when done. Generation only runs for
//! providers the user enabled, and the per-call cap and monthly budget reject whole batches.

use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use base64::Engine;
use serde_json::{Value, json};
use spark_core::{Document, ImageStatus, NodeId, Op, Parent, style::Fill};

use crate::{
    hub::Hub,
    images,
    providers::{GenUrl, ratio},
    secrets,
    settings::{ProviderSettings, Settings},
};

type Fut<'a> = Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + Send + 'a>>;

/// What a provider needs to make one image.
pub struct Request {
    pub prompt: String,
    pub width: u32,
    pub height: u32,
    pub aspect_ratio: String,
}

pub struct Ctx {
    pub client: reqwest::Client,
    pub base: String,
    pub key: Option<String>,
    pub model: String,
}

pub trait Provider: Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn default_model(&self) -> &'static str;
    fn default_base(&self) -> &'static str;
    /// Estimated USD per image, for the monthly budget.
    fn default_cost(&self) -> f64;
    fn needs_key(&self) -> bool {
        true
    }
    fn generate<'a>(&'a self, ctx: &'a Ctx, req: &'a Request) -> Fut<'a>;
}

async fn post_json(ctx: &Ctx, url: &str, body: &Value, auth: Option<(&str, String)>) -> Result<Value, String> {
    let mut r = ctx.client.post(url).json(body);
    if let Some((h, v)) = auth {
        r = r.header(h, v);
    }
    let res = r.send().await.map_err(|e| format!("request failed: {e}"))?;
    let status = res.status();
    let text = res.text().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let short: String = text.chars().take(300).collect();
        return Err(format!("HTTP {status}: {short}"));
    }
    serde_json::from_str(&text).map_err(|e| format!("unexpected response: {e}"))
}

fn b64(s: &str) -> Result<Vec<u8>, String> {
    let data = s.split_once(";base64,").map(|(_, d)| d).unwrap_or(s);
    base64::engine::general_purpose::STANDARD.decode(data.trim()).map_err(|e| format!("bad image data: {e}"))
}

fn key(ctx: &Ctx) -> String {
    ctx.key.clone().unwrap_or_default()
}

struct OpenAi;
impl Provider for OpenAi {
    fn id(&self) -> &'static str {
        "openai"
    }
    fn name(&self) -> &'static str {
        "OpenAI Images"
    }
    fn default_model(&self) -> &'static str {
        "gpt-image-1"
    }
    fn default_base(&self) -> &'static str {
        "https://api.openai.com"
    }
    fn default_cost(&self) -> f64 {
        0.04
    }
    fn generate<'a>(&'a self, ctx: &'a Ctx, req: &'a Request) -> Fut<'a> {
        Box::pin(async move {
            let size = match req.width.cmp(&req.height) {
                std::cmp::Ordering::Greater => "1536x1024",
                std::cmp::Ordering::Less => "1024x1536",
                std::cmp::Ordering::Equal => "1024x1024",
            };
            let body = json!({"model": ctx.model, "prompt": req.prompt, "size": size, "n": 1});
            let v = post_json(
                ctx,
                &format!("{}/v1/images/generations", ctx.base),
                &body,
                Some(("authorization", format!("Bearer {}", key(ctx)))),
            )
            .await?;
            match (v["data"][0]["b64_json"].as_str(), v["data"][0]["url"].as_str()) {
                (Some(d), _) => b64(d),
                (None, Some(u)) => images::fetch_url(u).await,
                _ => Err("the response had no image".into()),
            }
        })
    }
}

struct Gemini;
impl Provider for Gemini {
    fn id(&self) -> &'static str {
        "gemini"
    }
    fn name(&self) -> &'static str {
        "Google Gemini"
    }
    fn default_model(&self) -> &'static str {
        "gemini-2.5-flash-image"
    }
    fn default_base(&self) -> &'static str {
        "https://generativelanguage.googleapis.com"
    }
    fn default_cost(&self) -> f64 {
        0.04
    }
    fn generate<'a>(&'a self, ctx: &'a Ctx, req: &'a Request) -> Fut<'a> {
        Box::pin(async move {
            let body = json!({
                "contents": [{"parts": [{"text": req.prompt}]}],
                "generationConfig": {"responseModalities": ["IMAGE"], "imageConfig": {"aspectRatio": req.aspect_ratio}},
            });
            let url = format!("{}/v1beta/models/{}:generateContent", ctx.base, ctx.model);
            let v = post_json(ctx, &url, &body, Some(("x-goog-api-key", key(ctx)))).await?;
            let parts = v["candidates"][0]["content"]["parts"].as_array().cloned().unwrap_or_default();
            let data =
                parts.iter().find_map(|p| p["inlineData"]["data"].as_str().or(p["inline_data"]["data"].as_str()));
            b64(data.ok_or("the response had no image")?)
        })
    }
}

struct Fal;
impl Provider for Fal {
    fn id(&self) -> &'static str {
        "fal"
    }
    fn name(&self) -> &'static str {
        "fal.ai"
    }
    fn default_model(&self) -> &'static str {
        "fal-ai/flux/schnell"
    }
    fn default_base(&self) -> &'static str {
        "https://fal.run"
    }
    fn default_cost(&self) -> f64 {
        0.01
    }
    fn generate<'a>(&'a self, ctx: &'a Ctx, req: &'a Request) -> Fut<'a> {
        Box::pin(async move {
            let body = json!({"prompt": req.prompt, "image_size": {"width": req.width, "height": req.height}, "num_images": 1});
            let v = post_json(
                ctx,
                &format!("{}/{}", ctx.base, ctx.model),
                &body,
                Some(("authorization", format!("Key {}", key(ctx)))),
            )
            .await?;
            let url = v["images"][0]["url"].as_str().ok_or("the response had no image")?;
            if url.starts_with("data:") { b64(url) } else { images::fetch_url(url).await }
        })
    }
}

struct Replicate;
impl Provider for Replicate {
    fn id(&self) -> &'static str {
        "replicate"
    }
    fn name(&self) -> &'static str {
        "Replicate"
    }
    fn default_model(&self) -> &'static str {
        "black-forest-labs/flux-schnell"
    }
    fn default_base(&self) -> &'static str {
        "https://api.replicate.com"
    }
    fn default_cost(&self) -> f64 {
        0.01
    }
    fn generate<'a>(&'a self, ctx: &'a Ctx, req: &'a Request) -> Fut<'a> {
        Box::pin(async move {
            let auth = || Some(("authorization", format!("Bearer {}", key(ctx))));
            let body =
                json!({"input": {"prompt": req.prompt, "aspect_ratio": req.aspect_ratio, "output_format": "png"}});
            let mut v = ctx
                .client
                .post(format!("{}/v1/models/{}/predictions", ctx.base, ctx.model))
                .header("authorization", format!("Bearer {}", key(ctx)))
                .header("prefer", "wait")
                .json(&body)
                .send()
                .await
                .map_err(|e| e.to_string())?
                .json::<Value>()
                .await
                .map_err(|e| e.to_string())?;
            for _ in 0..120 {
                match v["status"].as_str() {
                    Some("succeeded") => break,
                    Some("failed" | "canceled") => return Err(format!("generation {}: {}", v["status"], v["error"])),
                    _ => {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        let get = v["urls"]["get"].as_str().ok_or("no prediction URL to poll")?.to_string();
                        let mut r = ctx.client.get(get);
                        if let Some((h, val)) = auth() {
                            r = r.header(h, val);
                        }
                        v = r.send().await.map_err(|e| e.to_string())?.json().await.map_err(|e| e.to_string())?;
                    }
                }
            }
            let out = if v["output"].is_array() { v["output"][0].clone() } else { v["output"].clone() };
            let url = out.as_str().ok_or("the prediction had no output")?;
            if url.starts_with("data:") { b64(url) } else { images::fetch_url(url).await }
        })
    }
}

struct OpenRouter;
impl Provider for OpenRouter {
    fn id(&self) -> &'static str {
        "openrouter"
    }
    fn name(&self) -> &'static str {
        "OpenRouter"
    }
    fn default_model(&self) -> &'static str {
        "google/gemini-2.5-flash-image"
    }
    fn default_base(&self) -> &'static str {
        "https://openrouter.ai"
    }
    fn default_cost(&self) -> f64 {
        0.04
    }
    fn generate<'a>(&'a self, ctx: &'a Ctx, req: &'a Request) -> Fut<'a> {
        Box::pin(async move {
            let body = json!({
                "model": ctx.model,
                "messages": [{"role": "user", "content": req.prompt}],
                "modalities": ["image", "text"],
                "image_config": {"aspect_ratio": req.aspect_ratio},
            });
            let v = post_json(
                ctx,
                &format!("{}/api/v1/chat/completions", ctx.base),
                &body,
                Some(("authorization", format!("Bearer {}", key(ctx)))),
            )
            .await?;
            let url = v["choices"][0]["message"]["images"][0]["image_url"]["url"]
                .as_str()
                .ok_or("the response had no image")?;
            if url.starts_with("data:") { b64(url) } else { images::fetch_url(url).await }
        })
    }
}

struct Automatic1111;
impl Provider for Automatic1111 {
    fn id(&self) -> &'static str {
        "automatic1111"
    }
    fn name(&self) -> &'static str {
        "Automatic1111 (local)"
    }
    fn default_model(&self) -> &'static str {
        ""
    }
    fn default_base(&self) -> &'static str {
        "http://127.0.0.1:7860"
    }
    fn default_cost(&self) -> f64 {
        0.0
    }
    fn needs_key(&self) -> bool {
        false
    }
    fn generate<'a>(&'a self, ctx: &'a Ctx, req: &'a Request) -> Fut<'a> {
        Box::pin(async move {
            let mut body = json!({"prompt": req.prompt, "width": req.width, "height": req.height, "steps": 25});
            if !ctx.model.is_empty() {
                body["override_settings"] = json!({"sd_model_checkpoint": ctx.model});
            }
            let v = post_json(ctx, &format!("{}/sdapi/v1/txt2img", ctx.base), &body, None).await?;
            b64(v["images"][0].as_str().ok_or("the response had no image")?)
        })
    }
}

struct ComfyUi;
impl Provider for ComfyUi {
    fn id(&self) -> &'static str {
        "comfyui"
    }
    fn name(&self) -> &'static str {
        "ComfyUI (local)"
    }
    fn default_model(&self) -> &'static str {
        "sd_xl_base_1.0.safetensors"
    }
    fn default_base(&self) -> &'static str {
        "http://127.0.0.1:8188"
    }
    fn default_cost(&self) -> f64 {
        0.0
    }
    fn needs_key(&self) -> bool {
        false
    }
    fn generate<'a>(&'a self, ctx: &'a Ctx, req: &'a Request) -> Fut<'a> {
        Box::pin(async move {
            // A minimal text-to-image graph: checkpoint → prompts → sampler → decode → save.
            let graph = json!({
                "1": {"class_type": "CheckpointLoaderSimple", "inputs": {"ckpt_name": ctx.model}},
                "2": {"class_type": "CLIPTextEncode", "inputs": {"text": req.prompt, "clip": ["1", 1]}},
                "3": {"class_type": "CLIPTextEncode", "inputs": {"text": "", "clip": ["1", 1]}},
                "4": {"class_type": "EmptyLatentImage", "inputs": {"width": req.width, "height": req.height, "batch_size": 1}},
                "5": {"class_type": "KSampler", "inputs": {"model": ["1", 0], "positive": ["2", 0], "negative": ["3", 0], "latent_image": ["4", 0], "seed": 1, "steps": 25, "cfg": 7.0, "sampler_name": "euler", "scheduler": "normal", "denoise": 1.0}},
                "6": {"class_type": "VAEDecode", "inputs": {"samples": ["5", 0], "vae": ["1", 2]}},
                "7": {"class_type": "SaveImage", "inputs": {"images": ["6", 0], "filename_prefix": "spark"}},
            });
            let v = post_json(ctx, &format!("{}/prompt", ctx.base), &json!({"prompt": graph}), None).await?;
            let id = v["prompt_id"].as_str().ok_or("ComfyUI didn't accept the prompt")?.to_string();
            for _ in 0..600 {
                tokio::time::sleep(Duration::from_millis(500)).await;
                let h: Value = ctx
                    .client
                    .get(format!("{}/history/{id}", ctx.base))
                    .send()
                    .await
                    .map_err(|e| e.to_string())?
                    .json()
                    .await
                    .map_err(|e| e.to_string())?;
                if let Some(img) = h[&id]["outputs"]["7"]["images"].get(0) {
                    let url = format!(
                        "{}/view?filename={}&subfolder={}&type={}",
                        ctx.base,
                        img["filename"].as_str().unwrap_or_default(),
                        img["subfolder"].as_str().unwrap_or_default(),
                        img["type"].as_str().unwrap_or("output")
                    );
                    return images::fetch_url(&url).await;
                }
            }
            Err("ComfyUI took too long".into())
        })
    }
}

pub static PROVIDERS: &[&dyn Provider] = &[&OpenAi, &Gemini, &Fal, &Replicate, &OpenRouter, &ComfyUi, &Automatic1111];

pub fn provider(id: &str) -> Option<&'static dyn Provider> {
    PROVIDERS.iter().copied().find(|p| p.id() == id)
}

fn config_of<'a>(settings: &'a Settings, id: &str) -> Option<&'a ProviderSettings> {
    settings.images.providers.get(id)
}

pub fn check_enabled(settings: &Settings, id: &str) -> Result<(), String> {
    let p = provider(id).ok_or_else(|| {
        format!(
            "unknown image provider `{id}`; use agent, {}",
            PROVIDERS.iter().map(|p| p.id()).collect::<Vec<_>>().join(", ")
        )
    })?;
    if !config_of(settings, id).is_some_and(|c| c.enabled) {
        return Err(format!(
            "image provider `{id}` is not enabled in Spark's settings; use spark-gen://agent to request an image instead"
        ));
    }
    if p.needs_key() && secrets::get(id).is_none() {
        return Err(format!("image provider `{id}` has no API key in Spark's settings"));
    }
    Ok(())
}

fn this_month() -> String {
    // Days since 1970 → civil date (Howard Hinnant's algorithm), enough for a YYYY-MM label.
    let days =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() / 86400).unwrap_or(0)
            as i64;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}")
}

pub fn cost(settings: &Settings, id: &str) -> f64 {
    config_of(settings, id)
        .and_then(|c| c.cost_per_image)
        .or_else(|| provider(id).map(|p| p.default_cost()))
        .unwrap_or(0.0)
}

/// Spent this month, resetting the counter when the month changes.
pub fn spent(settings: &Settings) -> f64 {
    if settings.images.spent.month == this_month() { settings.images.spent.amount } else { 0.0 }
}

/// The cost guard: a call may not exceed the per-call cap or push the month over budget.
pub fn guard(settings: &Settings, jobs: &[(String, GenUrl)]) -> Result<(), String> {
    let cap = settings.images.per_call_cap as usize;
    if jobs.len() > cap {
        return Err(format!(
            "this call asks for {} generated images but the limit is {cap} per call; nothing was generated",
            jobs.len()
        ));
    }
    if let Some(budget) = settings.images.monthly_budget {
        let total: f64 = jobs.iter().map(|(_, g)| cost(settings, &g.provider)).sum();
        let used = spent(settings);
        if used + total > budget + 1e-9 {
            return Err(format!(
                "these {} images (about ${total:.2}) would go over the monthly image budget (${used:.2} of ${budget:.2} used); nothing was generated",
                jobs.len()
            ));
        }
    }
    Ok(())
}

/// Runs the cost guard against the latest spend and charges the batch up front, so calls made at
/// the same time can't together go over budget. Returns the amount charged; failures refund.
pub fn charge(config: &crate::Config, jobs: &[(String, GenUrl)]) -> Result<f64, String> {
    let _g = crate::config::EDITS.lock().unwrap_or_else(|e| e.into_inner());
    let settings = Settings::load(config);
    guard(&settings, jobs)?;
    let total: f64 = jobs.iter().map(|(_, g)| cost(&settings, &g.provider)).sum();
    if total > 0.0 {
        add_spend(config, total);
    }
    Ok(total)
}

pub fn refund(config: &crate::Config, amount: f64) {
    if amount > 0.0 {
        let _g = crate::config::EDITS.lock().unwrap_or_else(|e| e.into_inner());
        add_spend(config, -amount);
    }
}

fn add_spend(config: &crate::Config, amount: f64) {
    let mut s = Settings::load(config);
    let month = this_month();
    if s.images.spent.month != month {
        s.images.spent = crate::settings::Spent { month, amount: 0.0 };
    }
    s.images.spent.amount = (s.images.spent.amount + amount).max(0.0);
    let _ = s.save(config);
}

/// The pixel size to request for an aspect ratio (long side 1024, multiples of 8).
pub fn size_for(r: &str) -> (u32, u32) {
    let (w, h) = ratio(r).unwrap_or((1.0, 1.0));
    let snap = |v: f64| ((v / 8.0).round() * 8.0) as u32;
    if w >= h { (1024, snap(1024.0 * h / w)) } else { (snap(1024.0 * w / h), 1024) }
}

/// Finds the node carrying an image request or job id.
pub fn node_for_request(doc: &Document, id: &str) -> Option<NodeId> {
    doc.all_nodes().find(|n| !n.deleted && n.image.as_ref().is_some_and(|i| i["id"] == id)).map(|n| n.id)
}

/// Text from reference nodes, prepended to the prompt as context.
fn reference_text(doc: &Document, refs: &[String]) -> String {
    let mut parts = Vec::new();
    for r in refs {
        let Ok(id) = r.parse::<NodeId>() else { continue };
        let mut texts: Vec<String> = Vec::new();
        for n in std::iter::once(id).chain(doc.descendants(Parent::Node(id))) {
            if let Some(t) = doc.node(n).and_then(|n| n.text.clone()) {
                texts.push(t);
            }
        }
        if !texts.is_empty() {
            parts.push(texts.join(" "));
        }
    }
    if parts.is_empty() { String::new() } else { format!("Context: {}\n\n", parts.join(" / ")) }
}

/// Marks images a file still calls `processing` as failed. Called when a file is loaded: the jobs
/// lived in an app that has since let go of the file, so nothing is left to finish them.
pub fn fail_interrupted(doc: &mut Document) {
    let cut_off: Vec<(NodeId, Value)> = doc
        .all_nodes()
        .filter_map(|n| Some((n.id, n.image.clone()?)))
        .filter(|(_, image)| ImageStatus::of(image) == ImageStatus::Processing)
        .collect();
    for (id, mut image) in cut_off {
        image["status"] = json!(ImageStatus::Error.as_str());
        image["error"] = json!("Spark closed this file before the image arrived.");
        let _ = doc.apply(Op::Set { id, prop: "image".into(), value: image });
    }
}

/// Puts finished image bytes on a request's node (or marks it failed). Not an undo step: the
/// placeholder's creation is.
pub fn finish(hub: &Hub, file: &str, request: &str, result: Result<Vec<u8>, String>) -> Result<NodeId, String> {
    let doc = hub.get(file).ok_or("the file was closed")?;
    let mut d = doc.lock().unwrap();
    // The placeholder may be deleted right now and come back with undo or redo: look past that.
    let node = d
        .editor
        .doc
        .all_nodes()
        .find(|n| n.image.as_ref().is_some_and(|i| i["id"] == request))
        .ok_or_else(|| format!("no image request {request}"))?;
    let (id, live) = (node.id, !node.deleted);
    let mut image = node.image.clone().unwrap_or(json!({}));
    let mut fills = node.style.fills.clone();
    match result {
        Ok(bytes) => {
            let asset = crate::add_asset(&mut d, &bytes)?;
            let hash = asset["hash"].as_str().unwrap().to_string();
            fills.retain(|f| !matches!(f, Fill::Image { .. }));
            fills.insert(0, Fill::Image { asset: hash, fit: spark_core::style::Fit::Cover, hidden: false });
            image["status"] = json!(ImageStatus::Ready.as_str());
            image["error"] = Value::Null;
        }
        Err(e) => {
            image["status"] = json!(ImageStatus::Error.as_str());
            image["error"] = json!(e);
        }
    }
    // History holds the node as it was inserted; without this, undo then redo brings back the
    // unfinished placeholder.
    d.editor.amend_inserts(id, |n| {
        n.image = Some(image.clone());
        n.style.fills = fills.clone();
    });
    if live {
        let ops = vec![
            Op::Set { id, prop: "style.fills".into(), value: serde_json::to_value(&fills).unwrap() },
            Op::Set { id, prop: "image".into(), value: image },
        ];
        let done = d.editor.apply_unrecorded(ops).map_err(|e| e.to_string())?;
        d.commit_ops(&done);
    }
    Ok(id)
}

/// Runs queued generation jobs for a file, one after another.
pub async fn run(hub: Arc<Hub>, file: String, jobs: Vec<(String, GenUrl)>) {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(300)).build().unwrap_or_default();
    for (request, g) in jobs {
        let settings = Settings::load(&hub.config);
        let result = async {
            let p = provider(&g.provider).ok_or("unknown provider")?;
            check_enabled(&settings, &g.provider)?;
            let cfg = config_of(&settings, p.id()).cloned().unwrap_or_default();
            let ctx = Ctx {
                client: client.clone(),
                base: cfg
                    .base_url
                    .filter(|b| !b.is_empty())
                    .unwrap_or_else(|| p.default_base().into())
                    .trim_end_matches('/')
                    .to_string(),
                key: secrets::get(p.id()),
                model: g
                    .model
                    .clone()
                    .or(cfg.model)
                    .filter(|m| !m.is_empty())
                    .unwrap_or_else(|| p.default_model().into()),
            };
            let context = hub
                .get(&file)
                .map(|d| reference_text(&d.lock().unwrap().editor.doc, &g.reference_nodes))
                .unwrap_or_default();
            let (width, height) = size_for(&g.aspect_ratio);
            let req = Request {
                prompt: format!("{context}{}", g.prompt),
                width,
                height,
                aspect_ratio: g.aspect_ratio.clone(),
            };
            let bytes = p.generate(&ctx, &req).await?;
            spark_core::asset::image_mime(&bytes).ok_or("the provider returned something that isn't an image")?;
            Ok::<_, String>(bytes)
        }
        .await;
        // The batch was charged up front; images that didn't come back are refunded.
        if result.is_err() {
            refund(&hub.config, cost(&settings, &g.provider));
        }
        let _ = finish(&hub, &file, &request, result);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(provider: &str) -> GenUrl {
        GenUrl {
            provider: provider.into(),
            model: None,
            prompt: "x".into(),
            aspect_ratio: "1:1".into(),
            reference_nodes: vec![],
        }
    }

    #[test]
    fn guard_rejects_whole_batches() {
        let mut s = Settings::default();
        let jobs: Vec<_> = (0..11).map(|i| (format!("{i}"), job("openai"))).collect();
        assert!(guard(&s, &jobs).unwrap_err().contains("limit is 10"));
        assert!(guard(&s, &jobs[..10]).is_ok());
        s.images.monthly_budget = Some(0.10);
        assert!(guard(&s, &jobs[..2]).is_ok(), "2 × $0.04 fits");
        assert!(guard(&s, &jobs[..3]).unwrap_err().contains("budget"));
        s.images.spent = crate::settings::Spent { month: this_month(), amount: 0.05 };
        assert!(guard(&s, &jobs[..2]).is_err());
        s.images.spent.month = "1999-01".into();
        assert!(guard(&s, &jobs[..2]).is_ok(), "last month's spend doesn't count");
    }

    #[test]
    fn charges_reserve_the_budget_until_refunded() {
        let dir = tempfile::tempdir().unwrap();
        let config = crate::Config::load(dir.path().join("config"), dir.path().join("files")).unwrap();
        let mut s = Settings::default();
        s.images.monthly_budget = Some(0.04);
        s.save(&config).unwrap();
        let one = [("a".to_string(), job("openai"))];
        assert!(charge(&config, &one).is_ok());
        assert!(charge(&config, &one).unwrap_err().contains("budget"), "the first image is still in flight");
        refund(&config, cost(&s, "openai"));
        assert!(charge(&config, &one).is_ok(), "a failed image gives its share back");
    }

    #[test]
    fn sizes_and_month() {
        assert_eq!(size_for("16:9"), (1024, 576));
        assert_eq!(size_for("9:16"), (576, 1024));
        assert_eq!(size_for("1:1"), (1024, 1024));
        assert_eq!(this_month().len(), 7);
        assert!(check_enabled(&Settings::default(), "openai").unwrap_err().contains("not enabled"));
        assert!(check_enabled(&Settings::default(), "nope").unwrap_err().contains("unknown"));
    }
}
