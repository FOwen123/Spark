//! Per-user settings that live in the platform config dir: the server token, the chosen port and
//! the recent-files list. Nothing here is ever written into a `.spark` file.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug)]
pub struct Config {
    /// Spark's config dir (token, port, recent files, settings).
    pub dir: PathBuf,
    /// Where new files are created.
    pub files_dir: PathBuf,
    pub token: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecentFile {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    pub opened_at: u64,
}

pub fn default_dir() -> PathBuf {
    std::env::var_os("SPARK_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::config_dir().unwrap_or_else(std::env::temp_dir).join("Spark"))
}

pub fn default_files_dir() -> PathBuf {
    std::env::var_os("SPARK_FILES_DIR").map(PathBuf::from).unwrap_or_else(|| {
        dirs::document_dir().or_else(dirs::home_dir).unwrap_or_else(std::env::temp_dir).join("Spark")
    })
}

/// Held across read-modify-write of Spark's and agents' config files, so concurrent edits in this
/// process don't drop each other's changes.
pub static EDITS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Writes a file atomically (temp file + rename). New files get mode 0600; existing files keep theirs.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = dir.join(format!(".{name}.{}.{n}.tmp", std::process::id()));
    let mut f = fs::File::create_new(&tmp)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(path).map(|m| m.permissions().mode() & 0o777).unwrap_or(0o600);
        f.set_permissions(fs::Permissions::from_mode(mode))?;
    }
    f.write_all(bytes)?;
    f.sync_all()?;
    drop(f);
    fs::rename(&tmp, path)
}

fn random_token() -> String {
    let mut bytes = [0u8; 24];
    getrandom::fill(&mut bytes).expect("os random");
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

impl Config {
    /// Loads the config dir, creating the token on first run.
    pub fn load(dir: PathBuf, files_dir: PathBuf) -> std::io::Result<Config> {
        fs::create_dir_all(&dir)?;
        let token_path = dir.join("token");
        let token = match fs::read_to_string(&token_path) {
            Ok(t) if t.trim().len() >= 32 => t.trim().to_string(),
            _ => {
                let t = random_token();
                write_private(&token_path, t.as_bytes())?;
                t
            }
        };
        Ok(Config { dir, files_dir, token })
    }

    pub fn write_port(&self, port: u16) -> std::io::Result<()> {
        write_private(&self.dir.join("port"), port.to_string().as_bytes())
    }

    pub fn recent(&self) -> Vec<RecentFile> {
        let mut list: Vec<RecentFile> = fs::read(self.dir.join("recent.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        list.retain(|f| f.path.exists());
        list
    }

    pub fn remember(&self, id: &str, name: &str, path: &Path) {
        let _edit = EDITS.lock().unwrap_or_else(|e| e.into_inner());
        let mut list = self.recent();
        list.retain(|f| f.id != id && f.path != path);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        list.insert(0, RecentFile { id: id.into(), name: name.into(), path: path.into(), opened_at: now });
        list.truncate(50);
        if let Ok(json) = serde_json::to_vec_pretty(&list) {
            let _ = write_private(&self.dir.join("recent.json"), &json);
        }
    }

    /// Moves a file (with any SQLite side files) into `Archive` in the files dir and takes it off
    /// the recent list. Returns where it went. The caller makes sure Spark isn't holding it open.
    pub fn archive(&self, id: &str, path: &Path) -> Result<PathBuf, String> {
        let _edit = EDITS.lock().unwrap_or_else(|e| e.into_inner());
        let dir = self.files_dir.join("Archive");
        let dest = if path.parent() == Some(dir.as_path()) {
            path.to_path_buf()
        } else {
            fs::create_dir_all(&dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
            let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into());
            let dest = (1..)
                .map(|i| dir.join(if i == 1 { format!("{stem}.spark") } else { format!("{stem} {i}.spark") }))
                .find(|p| !p.exists())
                .unwrap();
            let move_file = |from: &Path, to: &Path| {
                fs::rename(from, to).or_else(|_| fs::copy(from, to).and_then(|_| fs::remove_file(from)))
            };
            move_file(path, &dest).map_err(|e| format!("can't move {}: {e}", path.display()))?;
            for side in ["-wal", "-shm"] {
                let from = PathBuf::from(format!("{}{side}", path.display()));
                if from.exists() {
                    let _ = move_file(&from, Path::new(&format!("{}{side}", dest.display())));
                }
            }
            dest
        };
        let mut list = self.recent();
        list.retain(|f| f.id != id);
        if let Ok(json) = serde_json::to_vec_pretty(&list) {
            let _ = write_private(&self.dir.join("recent.json"), &json);
        }
        Ok(dest)
    }

    /// A path in the files dir that doesn't exist yet: `Untitled.spark`, `Untitled 2.spark`, …
    pub fn new_file_path(&self, name: &str) -> PathBuf {
        let clean: String = name.chars().map(|c| if "/\\:*?\"<>|".contains(c) { '-' } else { c }).collect();
        let clean = clean.trim();
        let base = if clean.is_empty() { "Untitled" } else { clean };
        (1..)
            .map(|i| self.files_dir.join(if i == 1 { format!("{base}.spark") } else { format!("{base} {i}.spark") }))
            .find(|p| !p.exists())
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_writes_leave_one_whole_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let (a, b) = (vec![b'a'; 1 << 20], vec![b'b'; 1 << 16]);
        for _ in 0..20 {
            std::thread::scope(|s| {
                s.spawn(|| write_private(&path, &a).unwrap());
                s.spawn(|| write_private(&path, &b).unwrap());
            });
            let got = fs::read(&path).unwrap();
            assert!(got == a || got == b, "the file is one write or the other, never a mix");
        }
    }

    #[test]
    fn token_is_created_once_with_private_mode() {
        let dir = tempfile::tempdir().unwrap();
        let a = Config::load(dir.path().join("cfg"), dir.path().join("files")).unwrap();
        let b = Config::load(dir.path().join("cfg"), dir.path().join("files")).unwrap();
        assert_eq!(a.token, b.token);
        assert_eq!(a.token.len(), 48);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.path().join("cfg/token")).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn recent_files_and_new_paths() {
        let dir = tempfile::tempdir().unwrap();
        let c = Config::load(dir.path().join("cfg"), dir.path().join("files")).unwrap();
        let p = c.new_file_path("Untitled");
        assert!(p.ends_with("Untitled.spark"));
        fs::create_dir_all(&c.files_dir).unwrap();
        fs::write(&p, b"x").unwrap();
        assert!(c.new_file_path("Untitled").ends_with("Untitled 2.spark"));
        c.remember("A", "Untitled", &p);
        c.remember("A", "Renamed", &p);
        let list = c.recent();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Renamed");
    }
}
