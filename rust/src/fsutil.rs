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
    match std::env::var_os("PACKAGER_TMP")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
    {
        Some(path) => absolute(&path),
        None => std::env::temp_dir(),
    }
}

/// Absolute path with the existing prefix canonicalized.
/// Missing trailing components are kept, so a not-yet-created output folder
/// can still be compared with the project directory.
pub fn normalize_path(path: &Path) -> PathBuf {
    let mut current = absolute(path);
    let mut missing = Vec::new();
    while !current.as_os_str().is_empty() && !current.exists() {
        match current.file_name() {
            Some(name) => {
                missing.push(name.to_os_string());
                if !current.pop() {
                    break;
                }
            }
            None => break,
        }
    }
    let mut canon = if current.as_os_str().is_empty() {
        PathBuf::from("/")
    } else {
        fs::canonicalize(&current).unwrap_or(current)
    };
    for name in missing.into_iter().rev() {
        canon.push(name);
    }
    canon
}

fn python_user_bins(home: &Path) -> Vec<PathBuf> {
    version_bins(&home.join("Library/Python"))
}

fn framework_python_bins() -> Vec<PathBuf> {
    version_bins(Path::new("/Library/Frameworks/Python.framework/Versions"))
}

fn version_bins(root: &Path) -> Vec<PathBuf> {
    let Ok(read) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut versions: Vec<PathBuf> = read
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter(|path| path.file_name().and_then(|name| name.to_str()) != Some("Current"))
        .collect();
    versions.sort_by_key(|path| std::cmp::Reverse(version_key(path)));
    versions
        .into_iter()
        .map(|version| version.join("bin"))
        .filter(|bin| bin.is_dir())
        .collect()
}

fn version_key(path: &Path) -> Vec<u32> {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let mut parts = Vec::new();
    let mut current = 0_u32;
    let mut in_number = false;
    for ch in name.chars() {
        if let Some(digit) = ch.to_digit(10) {
            current = current.saturating_mul(10).saturating_add(digit);
            in_number = true;
        } else if in_number {
            parts.push(current);
            current = 0;
            in_number = false;
        }
    }
    if in_number {
        parts.push(current);
    }
    parts
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
                let home = PathBuf::from(&home);
                for rel in [
                    ".cargo/bin",
                    "go/bin",
                    ".local/bin",
                    ".bun/bin",
                    ".dotnet",
                    ".dotnet/tools",
                ] {
                    let dir = home.join(rel);
                    if dir.is_dir() {
                        push(dir);
                    }
                }
                for dir in python_user_bins(&home) {
                    push(dir);
                }
            }
            for dir in framework_python_bins() {
                push(dir);
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
    find_tool(names).ok_or_else(|| PackagerError::msg(hint))
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
    copy_tree_inner(src, dst, skip, false)
}

/// Copy a project into a scratch directory. Build products from an earlier
/// run stay behind, and so does the directory the new product will occupy.
pub fn copy_project(src: &Path, dst: &Path, skip: &[PathBuf]) -> Result<()> {
    fs::create_dir_all(dst)?;
    copy_tree_inner(src, dst, skip, true)
}

pub struct StagedProject {
    _temp: TempDir,
    path: PathBuf,
}

impl StagedProject {
    pub fn stage(src: &Path, output_dir: &Path) -> Result<Self> {
        let temp = TempDir::new("packager-stage")?;
        let path = temp.path().join("src");
        let skip = [output_dir.to_path_buf()];
        copy_project(src, &path, &skip)?;
        Ok(Self { _temp: temp, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn copy_tree_inner(src: &Path, dst: &Path, skip: &[PathBuf], skip_build_dirs: bool) -> Result<()> {
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
        if skip_build_dirs
            && file_type.is_dir()
            && matches!(
                name_str.as_ref(),
                "target" | "build" | "obj" | "zig-cache" | ".zig-cache"
            )
        {
            continue;
        }
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
            copy_tree_inner(&from, &to, skip, skip_build_dirs)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_python_user_bin_comes_first() {
        let home = TempDir::new("python-bins").unwrap();
        for version in ["3.9", "3.10"] {
            fs::create_dir_all(home.path().join(format!("Library/Python/{version}/bin"))).unwrap();
        }
        let bins = python_user_bins(home.path());
        assert_eq!(bins.len(), 2);
        assert!(bins[0].ends_with("3.10/bin"), "{}", bins[0].display());
        assert!(bins[1].ends_with("3.9/bin"), "{}", bins[1].display());
    }
}
