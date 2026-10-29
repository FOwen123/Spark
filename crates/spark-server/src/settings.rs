//! Per-user settings (`settings.json` in the config dir): folders agents may read images from,
//! and image providers. Secrets never live here; provider keys go to the OS keychain.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{Config, write_private};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Folders whose images agents may place with spark-asset:// paths or insert_image.
    pub granted_dirs: Vec<PathBuf>,
    pub images: ImageSettings,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ImageSettings {
    /// A single tool call may request at most this many generated images.
    pub per_call_cap: u32,
    /// Optional monthly spending limit (USD, estimated from each provider's cost per image).
    pub monthly_budget: Option<f64>,
    pub spent: Spent,
    pub providers: std::collections::BTreeMap<String, ProviderSettings>,
}

impl Default for ImageSettings {
    fn default() -> Self {
        ImageSettings { per_call_cap: 10, monthly_budget: None, spent: Spent::default(), providers: Default::default() }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Spent {
    /// `YYYY-MM`.
    pub month: String,
    pub amount: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ProviderSettings {
    pub enabled: bool,
    pub model: Option<String>,
    /// Override the API base URL (local servers, proxies, compatible APIs).
    pub base_url: Option<String>,
    pub cost_per_image: Option<f64>,
}

impl Settings {
    pub fn load(config: &Config) -> Settings {
        std::fs::read(config.dir.join("settings.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, config: &Config) -> std::io::Result<()> {
        write_private(&config.dir.join("settings.json"), &serde_json::to_vec_pretty(self).unwrap())
    }

    /// Whether `path` is inside a granted folder (after resolving symlinks and `..`).
    pub fn is_granted(&self, path: &Path) -> bool {
        let Ok(real) = path.canonicalize() else { return false };
        self.granted_dirs.iter().filter_map(|d| d.canonicalize().ok()).any(|d| real.starts_with(d))
    }
}

pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg", "avif"];

/// Reads an image file an agent referenced, enforcing the granted folders and image types.
pub fn read_granted_image(settings: &Settings, path: &Path) -> Result<Vec<u8>, String> {
    if !path.is_absolute() {
        return Err(format!("{} is not an absolute path", path.display()));
    }
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
    if !IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        return Err(format!("{} is not an image file (png, jpg, gif, webp, svg or avif)", path.display()));
    }
    if !settings.is_granted(path) {
        return Err(format!(
            "{} is outside the folders Spark may read. Ask the user to grant its folder in Spark's settings.",
            path.display()
        ));
    }
    let meta = std::fs::metadata(path).map_err(|e| format!("can't read {}: {e}", path.display()))?;
    if meta.len() > 25 << 20 {
        return Err(format!("{} is larger than 25 MB", path.display()));
    }
    std::fs::read(path).map_err(|e| format!("can't read {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grants_limit_image_reads() {
        let dir = tempfile::tempdir().unwrap();
        let granted = dir.path().join("ok");
        let other = dir.path().join("no");
        std::fs::create_dir_all(&granted).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(granted.join("a.png"), b"png").unwrap();
        std::fs::write(other.join("b.png"), b"png").unwrap();
        std::fs::write(granted.join("c.txt"), b"txt").unwrap();
        let s = Settings { granted_dirs: vec![granted.clone()], ..Default::default() };
        assert!(read_granted_image(&s, &granted.join("a.png")).is_ok());
        assert!(read_granted_image(&s, &other.join("b.png")).unwrap_err().contains("outside"));
        assert!(read_granted_image(&s, &granted.join("c.txt")).is_err());
        assert!(read_granted_image(&s, &granted.join("../no/b.png")).is_err(), "no escaping with ..");
        assert!(read_granted_image(&s, Path::new("relative.png")).is_err());
    }
}
