use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::error::{PackagerError, Result};
use crate::fsutil::{self, absolute};
use crate::model::{Detection, Language};

const WALK_LIMIT: usize = 8;

pub fn resolve(path: &Path, language: Option<Language>) -> Result<Detection> {
    let path = absolute(path);
    if !path.exists() {
        return Err(PackagerError::msg(format!(
            "could not find {}",
            path.display()
        )));
    }
    let path = fs::canonicalize(&path).unwrap_or(path);
    match language {
        Some(language) => detect_forced(&path, language),
        None => detect(&path),
    }
}

fn detect(path: &Path) -> Result<Detection> {
    if path.is_file() {
        let name = fsutil::file_name(path);
        if name.ends_with(".csproj") || name.ends_with(".fsproj") || name.ends_with(".sln") {
            return Ok(found(
                Language::CSharp,
                path.to_path_buf(),
                parent(path),
                "C# project file",
            ));
        }
        if is_project_marker(&name) {
            return detect_dir(&parent(path));
        }
        if is_header(&name) {
            return Err(PackagerError::msg(
                "choose a source file rather than a header",
            ));
        }
        detect_file(path)
    } else if path.is_dir() {
        detect_dir(path)
    } else {
        Err(PackagerError::msg(format!(
            "{} is not a file or directory",
            path.display()
        )))
    }
}

fn detect_file(path: &Path) -> Result<Detection> {
    if let Some(language) = language_for_file(path) {
        let root = find_root(path, language).unwrap_or_else(|| parent(path));
        let reason = if root.join(marker_for(language).unwrap_or("")).is_file()
            || project_claims(language, &root)
        {
            format!(
                "{} file in a {} project",
                fsutil::file_name(path),
                language.label()
            )
        } else {
            format!(
                "file extension .{}",
                fsutil::extension_of(path).unwrap_or_default()
            )
        };
        return Ok(found(language, path.to_path_buf(), root, &reason));
    }
    if let Some(language) = shebang_language(path) {
        return Ok(found(
            language,
            path.to_path_buf(),
            parent(path),
            "script shebang",
        ));
    }
    Err(PackagerError::msg(format!(
        "could not tell which language {} is",
        path.display()
    )))
}

fn detect_dir(dir: &Path) -> Result<Detection> {
    if let Some((language, reason)) = identify_dir(dir) {
        let entry = project_entry(dir, language)?;
        return Ok(found(language, entry, dir.to_path_buf(), &reason));
    }
    detect_loose_dir(dir)
}

fn detect_forced(path: &Path, language: Language) -> Result<Detection> {
    if path.is_file() && is_project_marker(&fsutil::file_name(path)) {
        let dir = parent(path);
        let entry = project_entry(&dir, language)?;
        return Ok(found(
            language,
            entry,
            dir,
            &format!("language set to {}", language.label()),
        ));
    }
    if path.is_file() {
        if file_matches(path, language) {
            let root = find_root(path, language).unwrap_or_else(|| parent(path));
            return Ok(found(
                language,
                path.to_path_buf(),
                root,
                &format!("language set to {}", language.label()),
            ));
        }
        return Err(PackagerError::msg(format!(
            "{} is not a {} source file",
            fsutil::file_name(path),
            language.label()
        )));
    }
    let entry = project_entry(path, language)?;
    Ok(found(
        language,
        entry,
        path.to_path_buf(),
        &format!("language set to {}", language.label()),
    ))
}

fn found(language: Language, entry: PathBuf, project_root: PathBuf, reason: &str) -> Detection {
    Detection {
        language,
        entry,
        project_root,
        reason: reason.to_string(),
    }
}

fn identify_dir(dir: &Path) -> Option<(Language, String)> {
    let mark = |name: &str| dir.join(name).is_file();
    if mark("Cargo.toml") {
        return Some((Language::Rust, "Cargo.toml".into()));
    }
    if mark("go.mod") {
        return Some((Language::Go, "go.mod".into()));
    }
    if mark("Package.swift") {
        return Some((Language::Swift, "Package.swift".into()));
    }
    if mark("build.zig") {
        return Some((Language::Zig, "build.zig".into()));
    }
    if mark("pubspec.yaml") {
        return Some((Language::Dart, "pubspec.yaml".into()));
    }
    if has_project_file(dir, &["csproj", "fsproj", "sln"]) {
        return Some((Language::CSharp, "C# project".into()));
    }
    if mark("pom.xml") {
        return Some((Language::Java, "pom.xml".into()));
    }
    if mark("build.gradle") || mark("build.gradle.kts") {
        let language = if kotlin_project(dir) {
            Language::Kotlin
        } else {
            Language::Java
        };
        return Some((language, "Gradle project".into()));
    }
    if mark("package.json") {
        let language = if typescript_project(dir) {
            Language::TypeScript
        } else {
            Language::JavaScript
        };
        return Some((language, "package.json".into()));
    }
    if mark("pyproject.toml")
        || mark("setup.py")
        || mark("setup.cfg")
        || mark("requirements.txt")
        || mark("Pipfile")
    {
        return Some((Language::Python, "Python project".into()));
    }
    if mark("Gemfile") {
        return Some((Language::Ruby, "Gemfile".into()));
    }
    if mark("composer.json") {
        return Some((Language::Php, "composer.json".into()));
    }
    if mark("CMakeLists.txt") {
        return Some((native_language(dir), "CMakeLists.txt".into()));
    }
    if mark("Makefile") || mark("makefile") || mark("GNUmakefile") {
        return Some((native_language(dir), "Makefile".into()));
    }
    None
}

fn project_entry(dir: &Path, language: Language) -> Result<PathBuf> {
    match language {
        Language::Rust | Language::Swift | Language::Zig | Language::Java | Language::Kotlin => {
            Ok(dir.to_path_buf())
        }
        Language::Go => go_entry(dir),
        Language::CSharp => Ok(only_or_dir(dir, &["csproj", "fsproj", "sln"])),
        Language::Dart => dart_entry(dir),
        Language::JavaScript | Language::TypeScript => node_entry(dir, language),
        Language::Python => preferred_file(
            dir,
            language,
            &[
                "main.py",
                "app.py",
                "__main__.py",
                "src/main.py",
                "src/app.py",
            ],
        ),
        Language::Ruby => preferred_file(dir, language, &["main.rb", "app.rb", "bin/main.rb"]),
        Language::Php => preferred_file(
            dir,
            language,
            &["index.php", "public/index.php", "main.php"],
        ),
        Language::Perl => preferred_file(dir, language, &["main.pl", "app.pl"]),
        Language::Lua => preferred_file(dir, language, &["main.lua", "init.lua"]),
        Language::Shell => preferred_file(dir, language, &["main.sh", "run.sh", "start.sh"]),
        Language::C | Language::Cpp => c_entry(dir, language),
    }
}

fn preferred_file(dir: &Path, language: Language, preferred: &[&str]) -> Result<PathBuf> {
    for name in preferred {
        let path = dir.join(name);
        if path.is_file() {
            return Ok(path);
        }
    }
    let files = fsutil::files_with_extensions(dir, language.extensions(), 2);
    match files.as_slice() {
        [only] => Ok(only.clone()),
        [] => Err(PackagerError::msg(format!(
            "no {} source file in {}",
            language.label(),
            dir.display()
        ))),
        many => Err(PackagerError::msg(format!(
            "several {} files in {}. Pick one, for example {}",
            language.label(),
            dir.display(),
            fsutil::file_name(&many[0])
        ))),
    }
}

fn c_entry(dir: &Path, language: Language) -> Result<PathBuf> {
    if dir.join("CMakeLists.txt").is_file()
        || dir.join("Makefile").is_file()
        || dir.join("makefile").is_file()
        || dir.join("GNUmakefile").is_file()
    {
        return Ok(dir.to_path_buf());
    }
    let files = fsutil::files_with_extensions(dir, language.extensions(), 1);
    match files.as_slice() {
        [] => Err(PackagerError::msg(format!(
            "no {} source file in {}",
            language.label(),
            dir.display()
        ))),
        [only] => Ok(only.clone()),
        _ => Ok(dir.to_path_buf()),
    }
}

fn node_entry(dir: &Path, language: Language) -> Result<PathBuf> {
    let package_path = dir.join("package.json");
    if package_path.is_file() {
        if let Ok(text) = fs::read_to_string(&package_path) {
            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                if let Some(path) = package_entry(dir, &value) {
                    return Ok(path);
                }
            }
        }
    }
    let names: &[&str] = if language == Language::TypeScript {
        &[
            "index.ts",
            "index.tsx",
            "src/index.ts",
            "src/index.tsx",
            "src/main.ts",
            "main.ts",
        ]
    } else {
        &[
            "index.js",
            "index.mjs",
            "src/index.js",
            "main.js",
            "app.js",
            "server.js",
        ]
    };
    for name in names {
        let path = dir.join(name);
        if path.is_file() {
            return Ok(path);
        }
    }
    let files = fsutil::files_with_extensions(dir, language.extensions(), 2);
    match files.as_slice() {
        [only] => Ok(only.clone()),
        [] => Err(PackagerError::msg(format!(
            "no {} entry file in {}. Set \"main\" in package.json or add index.js",
            language.label(),
            dir.display()
        ))),
        many => Err(PackagerError::msg(format!(
            "several {} files in {}. Set \"main\" in package.json or pick {}",
            language.label(),
            dir.display(),
            many[0].display()
        ))),
    }
}

fn package_entry(dir: &Path, package: &Value) -> Option<PathBuf> {
    if let Some(bin) = package.get("bin").and_then(Value::as_str) {
        let path = dir.join(bin);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Some(map) = package.get("bin").and_then(Value::as_object) {
        for value in map.values() {
            if let Some(rel) = value.as_str() {
                let path = dir.join(rel);
                if path.is_file() {
                    return Some(path);
                }
            }
        }
    }
    if let Some(main) = package.get("main").and_then(Value::as_str) {
        let path = dir.join(main);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

fn dart_entry(dir: &Path) -> Result<PathBuf> {
    if let Some(path) = fs::read_dir(dir.join("bin")).ok().and_then(|read| {
        read.flatten()
            .map(|entry| entry.path())
            .find(|path| fsutil::extension_of(path).as_deref() == Some("dart"))
    }) {
        return Ok(path);
    }
    for name in ["main.dart", "lib/main.dart", "bin/main.dart"] {
        let path = dir.join(name);
        if path.is_file() {
            return Ok(path);
        }
    }
    preferred_file(dir, Language::Dart, &["main.dart"])
}

fn detect_loose_dir(dir: &Path) -> Result<Detection> {
    let mut counts: Vec<(Language, Vec<PathBuf>)> = Vec::new();
    for language in Language::ALL {
        let files = fsutil::files_with_extensions(dir, language.extensions(), 1);
        if !files.is_empty() {
            counts.push((language, files));
        }
    }
    if counts.is_empty() {
        return Err(PackagerError::msg(format!(
            "no recognized source in {}",
            dir.display()
        )));
    }
    counts.sort_by_key(|(_, files)| std::cmp::Reverse(files.len()));
    if counts.len() > 1 && counts[0].1.len() == counts[1].1.len() {
        return Err(PackagerError::msg(format!(
            "this folder mixes {} and {}. Pick a file or set the language",
            counts[0].0.label(),
            counts[1].0.label()
        )));
    }
    let (language, files) = counts.remove(0);
    let entry = if files.len() == 1 {
        files[0].clone()
    } else if matches!(language, Language::C | Language::Cpp) {
        dir.to_path_buf()
    } else if let Some(main) = files.iter().find(|path| {
        path.file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| matches!(stem, "main" | "index" | "app"))
    }) {
        main.clone()
    } else {
        return Err(PackagerError::msg(format!(
            "several {} files in {}. Pick the one to package",
            language.label(),
            dir.display()
        )));
    };
    Ok(found(
        language,
        entry,
        dir.to_path_buf(),
        &format!("{} source", language.label()),
    ))
}

fn language_for_file(path: &Path) -> Option<Language> {
    let ext = fsutil::extension_of(path)?;
    Language::ALL
        .into_iter()
        .find(|language| language.extensions().contains(&ext.as_str()))
}

fn file_matches(path: &Path, language: Language) -> bool {
    let name = fsutil::file_name(path);
    if language == Language::CSharp && (name.ends_with(".csproj") || name.ends_with(".sln")) {
        return true;
    }
    language_for_file(path) == Some(language)
}

fn find_root(start: &Path, language: Language) -> Option<PathBuf> {
    let mut current = if start.is_file() {
        parent(start)
    } else {
        start.to_path_buf()
    };
    for _ in 0..WALK_LIMIT {
        if project_claims(language, &current) {
            return Some(current);
        }
        if !current.pop() {
            break;
        }
    }
    None
}

fn project_claims(language: Language, dir: &Path) -> bool {
    match language {
        Language::Rust => dir.join("Cargo.toml").is_file(),
        Language::Go => dir.join("go.mod").is_file(),
        Language::Swift => dir.join("Package.swift").is_file(),
        Language::Zig => dir.join("build.zig").is_file(),
        Language::Dart => dir.join("pubspec.yaml").is_file(),
        Language::CSharp => has_project_file(dir, &["csproj", "fsproj", "sln"]),
        Language::Java | Language::Kotlin => {
            dir.join("pom.xml").is_file()
                || dir.join("build.gradle").is_file()
                || dir.join("build.gradle.kts").is_file()
        }
        Language::JavaScript | Language::TypeScript => dir.join("package.json").is_file(),
        Language::Python => {
            dir.join("pyproject.toml").is_file()
                || dir.join("setup.py").is_file()
                || dir.join("setup.cfg").is_file()
                || dir.join("requirements.txt").is_file()
                || dir.join("Pipfile").is_file()
        }
        Language::Ruby => dir.join("Gemfile").is_file(),
        Language::Php => dir.join("composer.json").is_file(),
        Language::C | Language::Cpp => {
            dir.join("CMakeLists.txt").is_file()
                || dir.join("Makefile").is_file()
                || dir.join("makefile").is_file()
                || dir.join("GNUmakefile").is_file()
        }
        Language::Perl | Language::Lua | Language::Shell => false,
    }
}

fn marker_for(language: Language) -> Option<&'static str> {
    match language {
        Language::Rust => Some("Cargo.toml"),
        Language::Go => Some("go.mod"),
        Language::Swift => Some("Package.swift"),
        Language::JavaScript | Language::TypeScript => Some("package.json"),
        Language::Python => Some("pyproject.toml"),
        _ => None,
    }
}

fn is_project_marker(name: &str) -> bool {
    crate::model::is_project_marker(name)
}

fn go_entry(dir: &Path) -> Result<PathBuf> {
    if go_package_is_main(dir) {
        return Ok(dir.to_path_buf());
    }
    let cmd = dir.join("cmd");
    let mut packages = Vec::new();
    if cmd.is_dir() {
        if let Ok(read) = fs::read_dir(&cmd) {
            for entry in read.flatten() {
                let path = entry.path();
                if path.is_dir() && go_package_is_main(&path) {
                    packages.push(path);
                }
            }
        }
    }
    packages.sort();
    if packages.len() == 1 {
        Ok(packages.remove(0))
    } else {
        Ok(dir.to_path_buf())
    }
}

fn go_package_is_main(dir: &Path) -> bool {
    fsutil::files_with_extensions(dir, &["go"], 1)
        .into_iter()
        .filter(|path| !fsutil::file_name(path).ends_with("_test.go"))
        .any(|path| {
            fs::read_to_string(path)
                .ok()
                .is_some_and(|text| text.lines().any(|line| line.trim() == "package main"))
        })
}

fn is_header(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    matches!(
        lower.rsplit_once('.').map(|(_, ext)| ext),
        Some("h" | "hpp" | "hh" | "hxx")
    )
}

fn parent(path: &Path) -> PathBuf {
    path.parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf()
}

fn typescript_project(dir: &Path) -> bool {
    if dir.join("tsconfig.json").is_file() {
        return true;
    }
    if let Ok(text) = fs::read_to_string(dir.join("package.json")) {
        if text.contains("\"typescript\"") {
            return true;
        }
    }
    !fsutil::files_with_extensions(dir, Language::TypeScript.extensions(), 2).is_empty()
}

fn kotlin_project(dir: &Path) -> bool {
    let kotlin = !fsutil::files_with_extensions(dir, &["kt"], 2).is_empty();
    let java = !fsutil::files_with_extensions(dir, &["java"], 2).is_empty();
    kotlin && !java
}

fn native_language(dir: &Path) -> Language {
    if fsutil::files_with_extensions(dir, Language::Cpp.extensions(), 3).is_empty() {
        Language::C
    } else {
        Language::Cpp
    }
}

fn has_project_file(dir: &Path, extensions: &[&str]) -> bool {
    fs::read_dir(dir).ok().is_some_and(|read| {
        read.flatten().any(|entry| {
            fsutil::extension_of(&entry.path())
                .is_some_and(|ext| extensions.contains(&ext.as_str()))
        })
    })
}

fn only_or_dir(dir: &Path, extensions: &[&str]) -> PathBuf {
    let mut matches = Vec::new();
    if let Ok(read) = fs::read_dir(dir) {
        for entry in read.flatten() {
            let path = entry.path();
            if fsutil::extension_of(&path).is_some_and(|ext| extensions.contains(&ext.as_str())) {
                matches.push(path);
            }
        }
    }
    if matches.len() == 1 {
        matches.remove(0)
    } else {
        dir.to_path_buf()
    }
}

fn shebang_language(path: &Path) -> Option<Language> {
    let text = fs::read_to_string(path).ok()?;
    let line = text.lines().next()?.trim();
    let rest = line.strip_prefix("#!")?.trim();
    let token = shebang_token(rest)?;
    language_for_interpreter(&token)
}

fn shebang_token(rest: &str) -> Option<String> {
    let mut parts = rest.split_whitespace();
    let first = parts.next()?;
    let base = Path::new(first).file_name()?.to_string_lossy().to_string();
    if base == "env" {
        for part in parts {
            if part.starts_with('-') {
                continue;
            }
            return Some(Path::new(part).file_name()?.to_string_lossy().to_string());
        }
        return None;
    }
    Some(base)
}

fn language_for_interpreter(token: &str) -> Option<Language> {
    let token = token.to_ascii_lowercase();
    if token.starts_with("python") {
        return Some(Language::Python);
    }
    if token.starts_with("ruby") {
        return Some(Language::Ruby);
    }
    if token == "node" || token == "nodejs" {
        return Some(Language::JavaScript);
    }
    if token.starts_with("php") {
        return Some(Language::Php);
    }
    if token.starts_with("perl") {
        return Some(Language::Perl);
    }
    if token == "lua" || token == "luajit" {
        return Some(Language::Lua);
    }
    if matches!(token.as_str(), "bash" | "zsh" | "sh" | "dash" | "ksh") {
        return Some(Language::Shell);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> crate::fsutil::TempDir {
        crate::fsutil::TempDir::new(name).unwrap()
    }

    #[test]
    fn detects_python_file_and_cargo_project() {
        let dir = scratch("detect-py");
        let file = dir.path().join("main.py");
        fs::write(&file, "print('hi')\n").unwrap();
        let detected = resolve(&file, None).unwrap();
        assert_eq!(detected.language, Language::Python);
        assert_eq!(detected.entry, fs::canonicalize(&file).unwrap());

        let rust_dir = scratch("detect-rs");
        fs::write(
            rust_dir.path().join("Cargo.toml"),
            "[package]\nname='demo'\n",
        )
        .unwrap();
        fs::create_dir(rust_dir.path().join("src")).unwrap();
        let source = rust_dir.path().join("src/main.rs");
        fs::write(&source, "fn main() {}\n").unwrap();
        let detected = resolve(&source, None).unwrap();
        assert_eq!(detected.language, Language::Rust);
        assert_eq!(
            detected.project_root,
            fs::canonicalize(rust_dir.path()).unwrap()
        );
    }

    #[test]
    fn detects_node_typescript_and_refuses_a_tie() {
        let node = scratch("detect-node");
        fs::write(
            node.path().join("package.json"),
            r#"{"name":"demo","main":"src/index.ts"}"#,
        )
        .unwrap();
        fs::create_dir(node.path().join("src")).unwrap();
        fs::write(node.path().join("src/index.ts"), "export {}\n").unwrap();
        fs::write(node.path().join("tsconfig.json"), "{}\n").unwrap();
        let detected = resolve(node.path(), None).unwrap();
        assert_eq!(detected.language, Language::TypeScript);
        assert!(detected.entry.ends_with("src/index.ts"));

        let mixed = scratch("detect-mixed");
        fs::write(mixed.path().join("a.py"), "print(1)\n").unwrap();
        fs::write(mixed.path().join("b.rb"), "puts 1\n").unwrap();
        assert!(resolve(mixed.path(), None).is_err());
    }

    #[test]
    fn shebang_matches_bash_and_not_fish() {
        let dir = scratch("shebang");
        let bash = dir.path().join("run");
        fs::write(&bash, "#!/usr/bin/env bash\necho hi\n").unwrap();
        assert_eq!(resolve(&bash, None).unwrap().language, Language::Shell);

        let direct = dir.path().join("direct");
        fs::write(&direct, "#!/bin/bash\necho hi\n").unwrap();
        assert_eq!(resolve(&direct, None).unwrap().language, Language::Shell);

        let fish = dir.path().join("swim");
        fs::write(&fish, "#!/usr/bin/env fish\necho hi\n").unwrap();
        assert!(resolve(&fish, None).is_err());
    }

    #[test]
    fn go_module_uses_the_single_cmd_package() {
        let dir = scratch("go-cmd");
        fs::write(dir.path().join("go.mod"), "module example.com/demo\n").unwrap();
        fs::write(dir.path().join("lib.go"), "package demo\n").unwrap();
        let cmd = dir.path().join("cmd/demo");
        fs::create_dir_all(&cmd).unwrap();
        fs::write(cmd.join("main.go"), "package main\nfunc main() {}\n").unwrap();
        let detected = resolve(dir.path(), None).unwrap();
        assert_eq!(detected.language, Language::Go);
        assert_eq!(detected.entry, fs::canonicalize(&cmd).unwrap());
    }
}
