use std::path::Path;

use crate::error::{PackagerError, Result};
use crate::fsutil::{self, TempDir};
use crate::model::{BuildOutcome, BuildRequest, Detection, Language, Toolchain};
use crate::process::{self, Log};
use crate::product;

pub fn toolchain() -> Toolchain {
    match fsutil::find_tool(&["swift", "swiftc"]) {
        Some(tool) => Toolchain {
            language: "swift".into(),
            label: "Swift".into(),
            available: true,
            tool: "swift".into(),
            detail: tool.display().to_string(),
        },
        None => Toolchain {
            language: "swift".into(),
            label: "Swift".into(),
            available: false,
            tool: "swift".into(),
            detail: "Install Xcode or the Swift toolchain".into(),
        },
    }
}

pub fn build(
    request: &BuildRequest,
    detection: &Detection,
    display: &str,
    log: &mut dyn Log,
) -> Result<BuildOutcome> {
    if detection.project_root.join("Package.swift").is_file() {
        return package_build(request, detection, display, log);
    }
    file_build(request, detection, display, log)
}

fn package_build(
    request: &BuildRequest,
    detection: &Detection,
    display: &str,
    log: &mut dyn Log,
) -> Result<BuildOutcome> {
    let swift = fsutil::require_tool(&["swift"], "Install Xcode or the Swift toolchain")?;
    let manifest = fsutil::read_to_string(&detection.project_root.join("Package.swift"))?;
    let expected = executable_targets(&manifest);
    let temp = TempDir::new("packager-swift")?;
    process::run_ok(
        &swift,
        &[
            "build".into(),
            "-c".into(),
            "release".into(),
            "--package-path".into(),
            detection.project_root.display().to_string(),
            "--scratch-path".into(),
            temp.path().display().to_string(),
        ],
        Some(&detection.project_root),
        &[],
        log,
    )?;
    let mut binaries = Vec::new();
    if !expected.is_empty() {
        for exe in fsutil::executables_under(temp.path(), 5) {
            if expected.iter().any(|name| fsutil::file_name(&exe) == *name) {
                binaries.push(exe);
            }
        }
    }
    if binaries.is_empty() {
        binaries = fsutil::executables_under(temp.path(), 5)
            .into_iter()
            .filter(|path| {
                let name = fsutil::file_name(path);
                !name.starts_with("swift-") && !name.contains("build.db")
            })
            .collect();
    }
    if binaries.is_empty() {
        return Err(PackagerError::msg(
            "swift build finished without an executable product. Add an .executableTarget.",
        ));
    }
    let mut artifacts = Vec::new();
    let mut notes = vec!["Native Swift executable.".to_string()];
    let several = binaries.len() > 1;
    for binary in binaries {
        let name = if several {
            format!("{} {}", display, fsutil::file_name(&binary))
        } else {
            display.to_string()
        };
        let delivered = product::deliver_binary(&binary, request, &name, log)?;
        artifacts.extend(delivered.artifacts);
        notes.extend(delivered.notes);
    }
    Ok(super::outcome(
        Language::Swift,
        artifacts,
        "swift build",
        notes,
    ))
}

fn file_build(
    request: &BuildRequest,
    detection: &Detection,
    display: &str,
    log: &mut dyn Log,
) -> Result<BuildOutcome> {
    let swiftc = fsutil::require_tool(&["swiftc"], "Install Xcode or the Swift toolchain")?;
    let source = super::source_file(&detection.entry)?;
    let contents = fsutil::read_to_string(&source)?;
    let temp = TempDir::new("packager-swiftc")?;
    let binary = temp.path().join("app");
    let mut args = Vec::new();
    if contents.contains("@main") {
        args.push("-parse-as-library".into());
    }
    args.push("-O".into());
    if contents.contains("import SwiftUI") {
        args.push("-framework".into());
        args.push("SwiftUI".into());
    }
    if contents.contains("import AppKit") || contents.contains("import Cocoa") {
        args.push("-framework".into());
        args.push("AppKit".into());
    }
    args.extend([
        "-o".into(),
        binary.display().to_string(),
        source.display().to_string(),
    ]);
    process::run_ok(
        &swiftc,
        &args,
        Some(source.parent().unwrap_or(Path::new("."))),
        &[],
        log,
    )?;
    let delivered = product::deliver_binary(&binary, request, display, log)?;
    Ok(super::finish(
        Language::Swift,
        delivered,
        "swiftc",
        vec!["Native Swift executable.".into()],
    ))
}

fn executable_targets(manifest: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = manifest;
    while let Some(index) = rest.find("executableTarget") {
        rest = &rest[index + "executableTarget".len()..];
        if let Some(name) = quoted_field(rest, "name") {
            names.push(name);
        }
    }
    names
}

fn quoted_field(text: &str, field: &str) -> Option<String> {
    let window = &text[..text.len().min(240)];
    let key = window.find(&format!("{field}:"))?;
    let after = &window[key + field.len() + 1..];
    let start = after.find('"')?;
    let rest = &after[start + 1..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}
