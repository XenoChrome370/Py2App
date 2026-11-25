use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{PackagerError, Result};

static TEMP_SEQ: AtomicU64 = AtomicU64::new(1);
static ENRICHED_PATH: OnceLock<std::ffi::OsString> = OnceLock::new();

pub struct TempDir {
    path: PathBuf,
    keep: bool,
}

impl TempDir {
    pub fn new(prefix: &str) -> Result<Self> {
        let seq = TEMP_SEQ.fetch_add(1, Ordering::Relaxed);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let mut path = temp_root();
        path.push(format!("{prefix}-{}-{seq}-{nanos}", std::process::id()));
        fs::create_dir_all(&path).map_err(|err| {
            PackagerError::msg(format!("could not create {}: {err}", path.display()))
        })?;
        let keep = std::env::var("PACKAGER_KEEP_TEMP").ok().as_deref() == Some("1");
        Ok(Self { path, keep })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn temp_root() -> PathBuf {
    std::env::var_os("PACKAGER_TMP")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(std::env::temp_dir)
}

pub fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

pub fn enriched_path() -> std::ffi::OsString {
    ENRICHED_PATH
        .get_or_init(|| {
            let mut seen = HashSet::new();
            let mut dirs = Vec::new();
            let mut push = |dir: PathBuf| {
                if dir.as_os_str().is_empty() {
                    return;
                }
                if seen.insert(dir.clone()) {
                    dirs.push(dir);
                }
            };
            if let Some(path) = std::env::var_os("PATH") {
                for dir in std::env::split_paths(&path) {
                    push(dir);
                }
            }
            if let Some(home) = std::env::var_os("HOME") {
                let home = PathBuf::from(home);
                for rel in [
                    ".cargo/bin",
                    "go/bin",
                    ".local/bin",
                    ".bun/bin",
                    ".dotnet",
                    ".dotnet/tools",
                    "Library/Python/3.9/bin",
                    "Library/Python/3.11/bin",
                    "Library/Python/3.12/bin",
                    "Library/Python/3.13/bin",
                    "Library/Python/3.14/bin",
                ] {
                    let dir = home.join(rel);
                    if dir.is_dir() {
                        push(dir);
                    }
                }
            }
            for dir in [
                "/opt/homebrew/bin",
                "/usr/local/bin",
                "/usr/bin",
                "/bin",
                "/usr/sbin",
                "/sbin",
            ] {
                let dir = PathBuf::from(dir);
                if dir.is_dir() {
                    push(dir);
                }
            }
            std::env::join_paths(dirs).unwrap_or_else(|_| std::ffi::OsString::from("/usr/bin:/bin"))
        })
        .clone()
}

pub fn is_executable(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)?.permissions();
        permissions.set_mode(permissions.mode() | 0o755);
        fs::set_permissions(path, permissions)?;
    }
    Ok(())
}

pub fn find_tool(names: &[&str]) -> Option<PathBuf> {
    let path = enriched_path();
    let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
    for name in names {
        for dir in &dirs {
            let candidate = dir.join(name);
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn require_tool(names: &[&str], hint: &str) -> Result<PathBuf> {
    find_tool(names).ok_or_else(|| PackagerError::msg(hint.to_string()))
}

pub fn write_file(path: &Path, contents: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = File::create(path)?;
    file.write_all(contents.as_bytes())?;
    Ok(())
}

pub fn read_to_string(path: &Path) -> Result<String> {
    let mut file = File::open(path)
        .map_err(|err| PackagerError::msg(format!("could not read {}: {err}", path.display())))?;
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    Ok(text)
}

pub fn copy_file(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(from, to).map_err(|err| {
        PackagerError::msg(format!(
            "could not copy {} to {}: {err}",
            from.display(),
            to.display()
        ))
    })?;
    Ok(())
}

pub fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    copy_tree_excluding(src, dst, &[])
}

pub fn copy_tree_excluding(src: &Path, dst: &Path, skip: &[PathBuf]) -> Result<()> {
    fs::create_dir_all(dst)?;
    copy_tree_inner(src, dst, skip)
}

fn copy_tree_inner(src: &Path, dst: &Path, skip: &[PathBuf]) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if matches!(
            name_str.as_ref(),
            ".git" | ".hg" | ".svn" | ".DS_Store" | "__pycache__" | ".packager-tmp"
        ) {
            continue;
        }
        let from = entry.path();
        if from == dst || dst.starts_with(&from) {
            continue;
        }
        if skip
            .iter()
            .any(|item| from == *item || from.starts_with(item))
        {
            continue;
        }
        let to = dst.join(&name);
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            #[cfg(unix)]
            {
                let target = fs::read_link(&from)?;
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent)?;
                }
                std::os::unix::fs::symlink(target, &to)?;
            }
            continue;
        }
        if file_type.is_dir() {
            fs::create_dir_all(&to)?;
            copy_tree_inner(&from, &to, skip)?;
        } else if file_type.is_file() {
            copy_file(&from, &to)?;
        }
    }
    Ok(())
}

pub fn extension_of(path: &Path) -> Option<String> {
    path.extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
}

pub fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}

pub fn has_shebang(path: &Path) -> bool {
    let Ok(mut file) = File::open(path) else {
        return false;
    };
    let mut buf = [0_u8; 2];
    file.read(&mut buf).ok() == Some(2) && &buf == b"#!"
}

pub fn files_with_extensions(dir: &Path, extensions: &[&str], depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk_ext(dir, extensions, depth, &mut found);
    found.sort();
    found
}

fn walk_ext(dir: &Path, extensions: &[&str], depth: usize, found: &mut Vec<PathBuf>) {
    if depth == 0 {
        return;
    }
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') || name_str == "node_modules" || name_str == "target" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            walk_ext(&path, extensions, depth - 1, found);
        } else if let Some(ext) = extension_of(&path) {
            if extensions.iter().any(|candidate| *candidate == ext) {
                found.push(path);
            }
        }
    }
}

pub fn executables_under(dir: &Path, depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    walk_exec(dir, depth, &mut found);
    found.sort();
    found
}

fn walk_exec(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if depth == 0 || !dir.is_dir() {
        return;
    }
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') || name_str == "node_modules" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            if name_str.ends_with(".dSYM") || name_str == "incremental" || name_str == "ModuleCache"
            {
                continue;
            }
            walk_exec(&path, depth - 1, found);
            continue;
        }
        if !is_executable(&path) {
            continue;
        }
        if let Some(ext) = extension_of(&path) {
            if matches!(
                ext.as_str(),
                "d" | "rlib"
                    | "rmeta"
                    | "dylib"
                    | "so"
                    | "a"
                    | "o"
                    | "swiftmodule"
                    | "swiftdoc"
                    | "json"
                    | "pcm"
                    | "dia"
                    | "txt"
                    | "md"
            ) {
                continue;
            }
        }
        found.push(path);
    }
}

pub fn bash_single(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn quote_arg(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_string();
    }
    if value
        .chars()
        .any(|ch| ch.is_whitespace() || matches!(ch, '"' | '\'' | '\\' | '$'))
    {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

pub fn snapshot_names(dir: &Path) -> HashSet<String> {
    fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .collect()
}
