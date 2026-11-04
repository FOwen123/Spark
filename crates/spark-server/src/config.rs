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

/// Renames, or copies then removes when the destination is on another volume.
fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::rename(from, to).or_else(|_| fs::copy(from, to).and_then(|_| fs::remove_file(from)))
}

/// A `.spark` file's SQLite side files that exist, with their suffixes.
fn side_files(path: &Path) -> Vec<(PathBuf, &'static str)> {
    ["-wal", "-shm"]
        .into_iter()
        .map(|side| (PathBuf::from(format!("{}{side}", path.display())), side))
        .filter(|(p, _)| p.exists())
        .collect()
}

/// Moves a file (with any SQLite side files) into `dir`, renaming on a clash: `Name.spark`, `Name 2.spark`, …
fn move_into(path: &Path, dir: &Path) -> Result<PathBuf, String> {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Untitled".into());
    let dest = unique_path(dir, &stem);
    move_file(path, &dest).map_err(|e| format!("can't move {}: {e}", path.display()))?;
    for (from, side) in side_files(path) {
        let _ = move_file(&from, Path::new(&format!("{}{side}", dest.display())));
    }
    Ok(dest)
}

/// A `.spark` path in `dir` that doesn't exist yet: `base.spark`, `base 2.spark`, …
fn unique_path(dir: &Path, base: &str) -> PathBuf {
    (1..)
        .map(|i| dir.join(if i == 1 { format!("{base}.spark") } else { format!("{base} {i}.spark") }))
        .find(|p| !p.exists())
        .unwrap()
}

/// A folder name that is one plain path component.
fn valid_component(c: &str) -> bool {
    !c.is_empty() && !c.starts_with('.') && !c.chars().any(|ch| "/\\:*?\"<>|".contains(ch) || ch.is_control())
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
            move_into(path, &dir)?
        };
        self.forget(id);
        Ok(dest)
    }

    /// Moves a file (with any SQLite side files) to the system Trash, or into `SPARK_TRASH_DIR`
    /// when that's set (tests), and takes it off the recent list. The caller makes sure Spark
    /// isn't holding it open.
    pub fn trash(&self, id: &str, path: &Path) -> Result<(), String> {
        let _edit = EDITS.lock().unwrap_or_else(|e| e.into_inner());
        let mut paths = vec![path.to_path_buf()];
        paths.extend(side_files(path).into_iter().map(|(p, _)| p));
        let failed = |e: String| format!("can't move {} to the Trash: {e}", path.display());
        if let Some(dir) = std::env::var_os("SPARK_TRASH_DIR").map(PathBuf::from) {
            fs::create_dir_all(&dir).map_err(|e| failed(e.to_string()))?;
            for p in &paths {
                move_file(p, &dir.join(p.file_name().unwrap_or_default())).map_err(|e| failed(e.to_string()))?;
            }
        } else {
            #[allow(unused_mut)]
            let mut ctx = trash::TrashContext::default();
            // The default asks Finder over Apple events, which can prompt for a permission.
            #[cfg(target_os = "macos")]
            trash::macos::TrashContextExtMacos::set_delete_method(&mut ctx, trash::macos::DeleteMethod::NsFileManager);
            ctx.delete_all(&paths).map_err(|e| failed(e.to_string()))?;
        }
        self.forget(id);
        Ok(())
    }

    fn forget(&self, id: &str) {
        let mut list = self.recent();
        list.retain(|f| f.id != id);
        if let Ok(json) = serde_json::to_vec_pretty(&list) {
            let _ = write_private(&self.dir.join("recent.json"), &json);
        }
    }

    /// A path in `dir` (the files dir when `None`) that doesn't exist yet: `Untitled.spark`, `Untitled 2.spark`, …
    pub fn new_file_path_in(&self, dir: Option<&Path>, name: &str) -> PathBuf {
        let clean: String = name.chars().map(|c| if "/\\:*?\"<>|".contains(c) { '-' } else { c }).collect();
        let clean = clean.trim();
        unique_path(dir.unwrap_or(&self.files_dir), if clean.is_empty() { "Untitled" } else { clean })
    }

    pub fn new_file_path(&self, name: &str) -> PathBuf {
        self.new_file_path_in(None, name)
    }

    /// A folder of the files dir, from a `/`-separated path relative to it (`""` is the files dir).
    /// Refuses anything that could leave the files dir, symlinks included.
    pub fn folder(&self, rel: &str) -> Result<PathBuf, String> {
        fs::create_dir_all(&self.files_dir).map_err(|e| e.to_string())?;
        let root = self.files_dir.canonicalize().map_err(|e| e.to_string())?;
        let mut dir = root.clone();
        for c in rel.split('/').filter(|c| !c.is_empty()) {
            if !valid_component(c) {
                return Err(format!("`{rel}` is not a folder in the files folder"));
            }
            dir.push(c);
        }
        let dir = dir.canonicalize().map_err(|_| format!("folder `{rel}` doesn't exist"))?;
        if !dir.starts_with(&root) || !dir.is_dir() {
            return Err(format!("`{rel}` is not a folder in the files folder"));
        }
        Ok(dir)
    }

    /// Where `path` sits in the files dir as a relative `/` path; `""` for the top or for files elsewhere.
    pub fn folder_of(&self, path: &Path) -> String {
        let root = self.files_dir.canonicalize().unwrap_or_else(|_| self.files_dir.clone());
        let rel = path.parent().and_then(|p| p.strip_prefix(&root).ok()).unwrap_or(Path::new(""));
        rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
    }

    /// Subfolders of a folder, by name. `Archive` stays out of the top level: archived files leave the recent list.
    pub fn subfolders(&self, rel: &str) -> Result<Vec<String>, String> {
        let mut names: Vec<String> = fs::read_dir(self.folder(rel)?)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| valid_component(n) && !(rel.is_empty() && n == "Archive"))
            .collect();
        names.sort_by_key(|n| n.to_lowercase());
        Ok(names)
    }

    /// Creates a folder inside `parent`; returns its relative path.
    pub fn mkdir(&self, parent: &str, name: &str) -> Result<String, String> {
        let name = name.trim();
        if !valid_component(name) {
            return Err(format!("`{name}` can't be a folder name"));
        }
        let dir = self.folder(parent)?.join(name);
        if dir.exists() {
            return Err(format!("`{name}` already exists"));
        }
        fs::create_dir(&dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
        Ok(format!("{}/{name}", parent.trim_matches('/')).trim_start_matches('/').to_string())
    }

    /// Moves a file (with its side files) into a folder of the files dir and keeps its place in the recent
    /// list. The caller makes sure Spark isn't holding it open.
    pub fn relocate(&self, id: &str, path: &Path, rel: &str) -> Result<PathBuf, String> {
        let dir = self.folder(rel)?;
        let _edit = EDITS.lock().unwrap_or_else(|e| e.into_inner());
        if path.parent() == Some(dir.as_path()) {
            return Ok(path.to_path_buf());
        }
        // Read the list first: `recent` drops entries whose file is gone, which the old path is once moved.
        let mut list = self.recent();
        let dest = move_into(path, &dir)?;
        for f in list.iter_mut().filter(|f| f.id == id) {
            f.path = dest.clone();
        }
        if let Ok(json) = serde_json::to_vec_pretty(&list) {
            let _ = write_private(&self.dir.join("recent.json"), &json);
        }
        Ok(dest)
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

    #[test]
    fn folders_stay_inside_the_files_dir() {
        let dir = tempfile::tempdir().unwrap();
        let c = Config::load(dir.path().join("cfg"), dir.path().join("files")).unwrap();
        assert_eq!(c.mkdir("", "Work").unwrap(), "Work");
        assert_eq!(c.mkdir("Work", "Logos").unwrap(), "Work/Logos");
        assert!(c.mkdir("", "Work").is_err());
        for bad in ["..", "a/b", ".hidden", "x\\y", ""] {
            assert!(c.mkdir("", bad).is_err(), "{bad}");
        }
        for bad in ["..", "Work/../..", "/etc", "Nope"] {
            assert!(c.folder(bad).is_err(), "{bad}");
        }
        assert_eq!(c.subfolders("").unwrap(), ["Work"]);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.path(), c.files_dir.join("Escape")).unwrap();
            assert!(c.folder("Escape").is_err());
            assert_eq!(c.subfolders("").unwrap(), ["Escape", "Work"]);
        }
        let file = c.files_dir.join("Work/Logos/a.spark");
        assert_eq!(
            c.folder_of(
                &file.canonicalize().unwrap_or_else(|_| c.files_dir.canonicalize().unwrap().join("Work/Logos/a.spark"))
            ),
            "Work/Logos"
        );
    }
}
