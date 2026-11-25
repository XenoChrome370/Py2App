use std::fs;
use std::path::Path;

use crate::detect;
use crate::error::{PackagerError, Result};
use crate::fsutil;
use crate::model::{
    display_name, language_catalog, BuildOutcome, BuildRequest, Language, LanguageInfo, OutputMode,
    Toolchain,
};
use crate::process::Log;
use crate::product::Delivery;

mod dart_lang;
mod dotnet;
mod go_lang;
mod jvm;
mod native;
mod node;
mod python;
mod rust_lang;
mod script;
mod swift_lang;
mod zig_lang;

pub fn languages() -> Vec<LanguageInfo> {
    language_catalog()
}

pub fn toolchains() -> Vec<Toolchain> {
    vec![
        python::toolchain(),
        rust_lang::toolchain(),
        go_lang::toolchain(),
        node::toolchain(Language::JavaScript),
        node::toolchain(Language::TypeScript),
        swift_lang::toolchain(),
        native::toolchain(Language::C),
        native::toolchain(Language::Cpp),
        jvm::toolchain(Language::Java),
        jvm::toolchain(Language::Kotlin),
        dotnet::toolchain(),
        script::toolchain(Language::Ruby),
        script::toolchain(Language::Php),
        script::toolchain(Language::Perl),
        script::toolchain(Language::Lua),
        script::toolchain(Language::Shell),
        zig_lang::toolchain(),
        dart_lang::toolchain(),
    ]
}

pub fn build(request: &BuildRequest, log: &mut dyn Log) -> Result<BuildOutcome> {
    let mut request = request.clone();
    request.source = fs::canonicalize(&request.source).unwrap_or(request.source);
    if let Some(icon) = &request.icon {
        if !icon.is_file() {
            return Err(PackagerError::msg(format!(
                "icon not found: {}",
                icon.display()
            )));
        }
        request.icon = Some(fs::canonicalize(icon).unwrap_or(icon.clone()));
    }

    let detection = detect::resolve(&request.source, request.language)?;
    let project =
        fs::canonicalize(&detection.project_root).unwrap_or(detection.project_root.clone());
    let output = fsutil::normalize_path(&request.output_dir);
    if output.starts_with(&project) {
        return Err(PackagerError::msg(format!(
            "output directory {} is inside the project {}. Choose a folder outside the source",
            output.display(),
            project.display()
        )));
    }
    if output.exists() && !output.is_dir() {
        return Err(PackagerError::msg(format!(
            "output is not a directory: {}",
            output.display()
        )));
    }
    fs::create_dir_all(&output).map_err(|err| {
        PackagerError::msg(format!("could not create {}: {err}", output.display()))
    })?;
    request.output_dir = fs::canonicalize(&output).unwrap_or(output);
    let display = display_name(&request, &detection);
    let kind = match request.mode {
        OutputMode::App => "an app",
        OutputMode::Tool => "a tool",
    };
    log.line(&format!(
        "Packaging {} as {kind} named \"{display}\"",
        detection.language.label()
    ));
    log.line(&format!("source {}", detection.entry.display()));
    log.line(&detection.reason);

    match detection.language {
        Language::Python => python::build(&request, &detection, &display, log),
        Language::Rust => rust_lang::build(&request, &detection, &display, log),
        Language::Go => go_lang::build(&request, &detection, &display, log),
        Language::JavaScript | Language::TypeScript => {
            node::build(&request, &detection, &display, log)
        }
        Language::Swift => swift_lang::build(&request, &detection, &display, log),
        Language::C | Language::Cpp => native::build(&request, &detection, &display, log),
        Language::Java | Language::Kotlin => jvm::build(&request, &detection, &display, log),
        Language::CSharp => dotnet::build(&request, &detection, &display, log),
        Language::Ruby | Language::Php | Language::Perl | Language::Lua | Language::Shell => {
            script::build(&request, &detection, &display, log)
        }
        Language::Zig => zig_lang::build(&request, &detection, &display, log),
        Language::Dart => dart_lang::build(&request, &detection, &display, log),
    }
}

pub(crate) fn finish(
    language: Language,
    delivered: Delivery,
    tool: &str,
    mut notes: Vec<String>,
) -> BuildOutcome {
    notes.extend(delivered.notes);
    outcome(language, delivered.artifacts, tool, notes)
}

pub(crate) fn outcome(
    language: Language,
    artifacts: Vec<std::path::PathBuf>,
    tool: &str,
    notes: Vec<String>,
) -> BuildOutcome {
    BuildOutcome {
        language,
        artifacts,
        tool: tool.to_string(),
        notes,
    }
}

pub(crate) fn source_file(path: &Path) -> Result<std::path::PathBuf> {
    if path.is_file() {
        Ok(path.to_path_buf())
    } else {
        Err(PackagerError::msg(format!(
            "expected a source file, found {}",
            path.display()
        )))
    }
}
