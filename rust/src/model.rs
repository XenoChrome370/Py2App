use std::path::{Path, PathBuf};

use serde::Serialize;

/// Languages the engine knows how to package.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Python,
    Rust,
    Go,
    JavaScript,
    TypeScript,
    Swift,
    C,
    Cpp,
    Java,
    Kotlin,
    CSharp,
    Ruby,
    Php,
    Perl,
    Lua,
    Shell,
    Zig,
    Dart,
}

impl Language {
    pub const ALL: [Language; 18] = [
        Language::Python,
        Language::Rust,
        Language::Go,
        Language::JavaScript,
        Language::TypeScript,
        Language::Swift,
        Language::C,
        Language::Cpp,
        Language::Java,
        Language::Kotlin,
        Language::CSharp,
        Language::Ruby,
        Language::Php,
        Language::Perl,
        Language::Lua,
        Language::Shell,
        Language::Zig,
        Language::Dart,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Language::Python => "python",
            Language::Rust => "rust",
            Language::Go => "go",
            Language::JavaScript => "javascript",
            Language::TypeScript => "typescript",
            Language::Swift => "swift",
            Language::C => "c",
            Language::Cpp => "cpp",
            Language::Java => "java",
            Language::Kotlin => "kotlin",
            Language::CSharp => "csharp",
            Language::Ruby => "ruby",
            Language::Php => "php",
            Language::Perl => "perl",
            Language::Lua => "lua",
            Language::Shell => "shell",
            Language::Zig => "zig",
            Language::Dart => "dart",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Language::Python => "Python",
            Language::Rust => "Rust",
            Language::Go => "Go",
            Language::JavaScript => "JavaScript",
            Language::TypeScript => "TypeScript",
            Language::Swift => "Swift",
            Language::C => "C",
            Language::Cpp => "C++",
            Language::Java => "Java",
            Language::Kotlin => "Kotlin",
            Language::CSharp => "C#",
            Language::Ruby => "Ruby",
            Language::Php => "PHP",
            Language::Perl => "Perl",
            Language::Lua => "Lua",
            Language::Shell => "Shell",
            Language::Zig => "Zig",
            Language::Dart => "Dart",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Language::Python => "py2app application, or a PyInstaller tool",
            Language::Rust => "cargo release build",
            Language::Go => "go build",
            Language::JavaScript => "bun, pkg, or a Node launcher",
            Language::TypeScript => "bun --compile",
            Language::Swift => "swift build or swiftc",
            Language::C => "clang, CMake, or Make",
            Language::Cpp => "clang++, CMake, or Make",
            Language::Java => "Maven, Gradle, javac, and jpackage",
            Language::Kotlin => "Gradle or kotlinc",
            Language::CSharp => "dotnet publish",
            Language::Ruby => "application that runs with Ruby",
            Language::Php => "application that runs with PHP",
            Language::Perl => "application that runs with Perl",
            Language::Lua => "application that runs with Lua",
            Language::Shell => "application that runs a shell script",
            Language::Zig => "zig build",
            Language::Dart => "dart compile exe",
        }
    }

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Language::Python => &["py", "pyw"],
            Language::Rust => &["rs"],
            Language::Go => &["go"],
            Language::JavaScript => &["js", "mjs", "cjs", "jsx"],
            Language::TypeScript => &["ts", "tsx"],
            Language::Swift => &["swift"],
            Language::C => &["c", "m"],
            Language::Cpp => &["cpp", "cc", "cxx", "mm", "c++"],
            Language::Java => &["java"],
            Language::Kotlin => &["kt", "kts"],
            Language::CSharp => &["cs"],
            Language::Ruby => &["rb"],
            Language::Php => &["php"],
            Language::Perl => &["pl", "pm"],
            Language::Lua => &["lua"],
            Language::Shell => &["sh", "bash", "zsh"],
            Language::Zig => &["zig"],
            Language::Dart => &["dart"],
        }
    }

    pub fn parse(raw: &str) -> Option<Language> {
        let key = raw.trim().to_ascii_lowercase();
        Language::ALL
            .into_iter()
            .find(|language| language.id() == key || language.aliases().contains(&key.as_str()))
    }

    fn aliases(self) -> &'static [&'static str] {
        match self {
            Language::Python => &["py"],
            Language::Rust => &["rs", "cargo"],
            Language::Go => &["golang"],
            Language::JavaScript => &["js", "node"],
            Language::TypeScript => &["ts"],
            Language::Cpp => &["c++", "cxx"],
            Language::Kotlin => &["kt"],
            Language::CSharp => &["cs", "c#", "dotnet"],
            Language::Ruby => &["rb"],
            Language::Perl => &["pl"],
            Language::Shell => &["bash", "sh", "zsh"],
            _ => &[],
        }
    }
}

/// A double-clickable macOS app, or a terminal program.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
    App,
    Tool,
}

impl OutputMode {
    pub fn parse(raw: &str) -> Option<OutputMode> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "app" | "application" | "bundle" => Some(OutputMode::App),
            "tool" | "cli" | "bin" | "binary" => Some(OutputMode::Tool),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct BuildRequest {
    pub source: PathBuf,
    pub output_dir: PathBuf,
    pub language: Option<Language>,
    pub name: Option<String>,
    pub mode: OutputMode,
    pub icon: Option<PathBuf>,
}

#[derive(Clone, Debug)]
pub struct Detection {
    pub language: Language,
    pub entry: PathBuf,
    pub project_root: PathBuf,
    pub reason: String,
}

#[derive(Clone, Debug)]
pub struct BuildOutcome {
    pub language: Language,
    pub artifacts: Vec<PathBuf>,
    pub tool: String,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct LanguageInfo {
    pub id: String,
    pub label: String,
    pub blurb: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct Toolchain {
    pub language: String,
    pub label: String,
    pub available: bool,
    pub tool: String,
    pub detail: String,
}

pub fn language_catalog() -> Vec<LanguageInfo> {
    Language::ALL
        .into_iter()
        .map(|language| LanguageInfo {
            id: language.id().to_string(),
            label: language.label().to_string(),
            blurb: language.blurb().to_string(),
        })
        .collect()
}

pub fn clean_display_name(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if ch.is_alphanumeric() || ch == ' ' || ch == '-' || ch == '_' || ch == '.' {
            out.push(ch);
        }
    }
    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = collapsed.trim_matches(|c: char| c == '.' || c == '-' || c == '_');
    let trimmed = trimmed.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        trimmed.chars().take(80).collect()
    }
}

pub fn executable_slug(display: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for ch in display.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.push(ch.to_ascii_lowercase());
        } else if ch.is_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            for lower in ch.to_lowercase() {
                slug.push(lower);
            }
        } else {
            pending_dash = true;
        }
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "app".to_string()
    } else {
        slug
    }
}

pub fn bundle_identifier(display: &str) -> String {
    format!("dev.packager.{}", executable_slug(display))
}

pub(crate) fn is_project_marker(name: &str) -> bool {
    matches!(
        name,
        "Cargo.toml"
            | "go.mod"
            | "Package.swift"
            | "build.zig"
            | "pubspec.yaml"
            | "package.json"
            | "pyproject.toml"
            | "setup.py"
            | "setup.cfg"
            | "requirements.txt"
            | "Pipfile"
            | "Pipfile.lock"
            | "Gemfile"
            | "composer.json"
            | "CMakeLists.txt"
            | "Makefile"
            | "makefile"
            | "GNUmakefile"
            | "pom.xml"
            | "build.gradle"
            | "build.gradle.kts"
            | "tsconfig.json"
    )
}

fn generic_stem(stem: &str) -> bool {
    matches!(
        stem.to_ascii_lowercase().as_str(),
        "main" | "index" | "app" | "__main__"
    )
}

fn directory_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string()
}

/// Name to show before the user has typed one.
/// Generic entry files such as `main.py` take the project folder's name.
fn entry_is_file(path: &Path) -> bool {
    if path.is_file() {
        return true;
    }
    if path.is_dir() {
        return false;
    }
    path.extension().is_some()
}

pub fn suggested_name(detection: &Detection) -> String {
    let raw = if entry_is_file(&detection.entry) {
        let stem = detection
            .entry
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let file_name = detection
            .entry
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        if generic_stem(&stem) || is_project_marker(&file_name) {
            directory_name(&detection.project_root)
        } else {
            stem
        }
    } else {
        directory_name(&detection.project_root)
    };
    let cleaned = clean_display_name(&raw);
    if cleaned.is_empty() {
        "App".to_string()
    } else {
        cleaned
    }
}

pub fn display_name(request: &BuildRequest, detection: &Detection) -> String {
    if let Some(name) = &request.name {
        let cleaned = clean_display_name(name);
        if !cleaned.is_empty() {
            return cleaned;
        }
    }
    let raw = if detection.entry.is_file() {
        detection
            .entry
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    } else {
        detection
            .project_root
            .file_name()
            .or_else(|| detection.entry.file_name())
            .unwrap_or_default()
            .to_string_lossy()
            .to_string()
    };
    let cleaned = clean_display_name(&raw);
    if cleaned.is_empty() {
        "App".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_language_aliases() {
        assert_eq!(Language::parse("c++"), Some(Language::Cpp));
        assert_eq!(Language::parse("dotnet"), Some(Language::CSharp));
        assert_eq!(Language::parse("nope"), None);
    }

    #[test]
    fn slugs_display_names() {
        assert_eq!(executable_slug("Hello World"), "hello-world");
        assert_eq!(clean_display_name("  Hello   World!! "), "Hello World");
        assert_eq!(bundle_identifier("Hello"), "dev.packager.hello");
    }

    #[test]
    fn suggests_the_folder_for_a_generic_entry() {
        let detection = Detection {
            language: Language::Python,
            entry: PathBuf::from("/tmp/demo/main.py"),
            project_root: PathBuf::from("/tmp/demo"),
            reason: String::new(),
        };
        assert_eq!(suggested_name(&detection), "demo");

        let named = Detection {
            language: Language::Python,
            entry: PathBuf::from("/tmp/demo/hello.py"),
            project_root: PathBuf::from("/tmp/demo"),
            reason: String::new(),
        };
        assert_eq!(suggested_name(&named), "hello");
    }
}
