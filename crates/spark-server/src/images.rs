//! Loading images referenced by HTML: `data:` URIs and http(s) URLs are fetched before the
//! document is locked, then handed to the HTML importer.

use std::{collections::HashMap, time::Duration};

use base64::Engine;
use scraper::{Html, Selector};
use spark_core::{
    asset::{asset_hash, image_mime},
    html::ImageSrc,
};

const MAX_IMAGE: usize = 25 << 20;

/// Image URLs used by `<img src>` and `url(...)` in inline styles.
pub fn image_urls(html: &str) -> Vec<String> {
    let doc = Html::parse_fragment(html);
    let mut out = Vec::new();
    for el in doc.select(&Selector::parse("[src], [style]").unwrap()) {
        if let Some(src) = el.value().attr("src") {
            out.push(src.trim().to_string());
        }
        if let Some(style) = el.value().attr("style") {
            let mut rest = style;
            while let Some(i) = rest.find("url(") {
                let after = &rest[i + 4..];
                let Some(end) = after.find(')') else { break };
                out.push(after[..end].trim().trim_matches(['"', '\'']).to_string());
                rest = &after[end + 1..];
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

pub type Loaded = Result<Vec<u8>, String>;

fn decode_data_uri(uri: &str) -> Loaded {
    let rest = uri.strip_prefix("data:").ok_or("not a data URI")?;
    let (meta, data) = rest.split_once(',').ok_or("malformed data URI")?;
    if meta.ends_with(";base64") {
        base64::engine::general_purpose::STANDARD
            .decode(data.trim().replace(['\n', '\r', ' '], ""))
            .map_err(|e| format!("bad base64 image data: {e}"))
    } else {
        let mut out = Vec::new();
        let b = data.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'%'
                && i + 2 < b.len()
                && let Ok(v) = u8::from_str_radix(&data[i + 1..i + 3], 16)
            {
                out.push(v);
                i += 3;
                continue;
            }
            out.push(b[i]);
            i += 1;
        }
        Ok(out)
    }
}

pub async fn fetch_text(url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(20)).build().map_err(|e| e.to_string())?;
    let res = client.get(url).send().await.map_err(|e| e.to_string())?;
    res.error_for_status().map_err(|e| e.to_string())?.text().await.map_err(|e| e.to_string())
}

pub async fn fetch_url(url: &str) -> Loaded {
    let client = reqwest::Client::builder().timeout(Duration::from_secs(20)).build().map_err(|e| e.to_string())?;
    let res = client.get(url).send().await.map_err(|e| format!("couldn't download {url}: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("couldn't download {url}: HTTP {}", res.status()));
    }
    let bytes = res.bytes().await.map_err(|e| format!("couldn't download {url}: {e}"))?;
    if bytes.len() > MAX_IMAGE {
        return Err(format!("{url} is larger than 25 MB"));
    }
    Ok(bytes.to_vec())
}

/// The local path in a `spark-asset:///absolute/path` URL (percent-decoded).
pub fn asset_path(url: &str) -> Option<std::path::PathBuf> {
    let rest = url.strip_prefix("spark-asset://")?;
    let bytes = rest.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(v) = u8::from_str_radix(&rest[i + 1..i + 3], 16)
        {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    Some(std::path::PathBuf::from(String::from_utf8_lossy(&out).to_string()))
}

/// Loads every URL that needs loading: data URIs, http(s) URLs and granted local files.
/// Other schemes (spark-gen) are left to the resolver.
pub async fn prefetch(urls: &[String], settings: &crate::settings::Settings) -> HashMap<String, Loaded> {
    let mut out = HashMap::new();
    for url in urls {
        if url.starts_with("data:") {
            out.insert(url.clone(), decode_data_uri(url));
        } else if url.starts_with("http://") || url.starts_with("https://") {
            out.insert(url.clone(), fetch_url(url).await);
        } else if let Some(path) = asset_path(url) {
            out.insert(url.clone(), crate::settings::read_granted_image(settings, &path));
        }
    }
    out
}

/// Turns loaded bytes into an importer answer (validating that they are an image).
pub fn to_src(url: &str, loaded: &Loaded) -> ImageSrc {
    match loaded {
        Err(e) => ImageSrc::Failed(e.clone()),
        Ok(bytes) => match image_mime(bytes) {
            None => ImageSrc::Failed(format!("{} is not a supported image", short(url))),
            Some(mime) => {
                let size = imagesize::blob_size(bytes).ok();
                ImageSrc::Asset {
                    hash: asset_hash(bytes),
                    mime: mime.into(),
                    bytes: Some(bytes.clone()),
                    width: size.map(|s| s.width as f64),
                    height: size.map(|s| s.height as f64),
                }
            }
        },
    }
}

fn short(url: &str) -> String {
    if url.chars().count() > 64 { format!("{}…", url.chars().take(64).collect::<String>()) } else { url.to_string() }
}

/// A resolver over prefetched images for [`spark_core::html::import`].
pub fn resolver(loaded: &HashMap<String, Loaded>) -> impl FnMut(&str) -> ImageSrc + '_ {
    move |url: &str| match loaded.get(url) {
        Some(l) => to_src(url, l),
        None => ImageSrc::Failed(format!("unsupported image URL {}", short(url))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_urls_and_decodes_data() {
        let urls = image_urls(r#"<img src="a.png"><div style="background: url('b.jpg') center / cover"></div>"#);
        assert_eq!(urls, vec!["a.png", "b.jpg"]);
        let png = decode_data_uri("data:image/png;base64,iVBORw0KGgo=").unwrap();
        assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
        assert_eq!(decode_data_uri("data:image/svg+xml,%3Csvg%3E").unwrap(), b"<svg>");
    }
}
