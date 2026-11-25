use std::path::Path;
use std::process::{Command, Stdio};

use serde_json::Value;

use crate::error::{PackagerError, Result};
use crate::fsutil::{self, TempDir};
use crate::model::{BuildOutcome, BuildRequest, Detection, Language, Toolchain};
use crate::process::{self, Log};
use crate::product;

pub fn toolchain() -> Toolchain {
    match fsutil::find_tool(&["cargo"]) {
        Some(cargo) => Toolchain {
            language: "rust".into(),
            label: "Rust".into(),
            available: true,
            tool: "cargo".into(),
            detail: cargo.display().to_string(),
        },
        None => Toolchain {
            language: "rust".into(),
            label: "Rust".into(),
            available: false,
            tool: "cargo".into(),
            detail: "Install Rust from https://rustup.rs".into(),
        },
    }
}

pub fn build(
    request: &BuildRequest,
    detection: &Detection,
    display: &str,
    log: &mut dyn Log,
) -> Result<BuildOutcome> {
    let manifest = detection.project_root.join("Cargo.toml");
    if manifest.is_file() {
        return cargo_build(request, detection, display, &manifest, log);
    }
    let source = super::source_file(&detection.entry)?;
    let rustc = fsutil::require_tool(&["rustc"], "Install Rust from https://rustup.rs")?;
    let temp = TempDir::new("packager-rustc")?;
    let binary = temp.path().join("app");
    process::run_ok(
        &rustc,
        &[
            "-O".into(),
            "-o".into(),
            binary.display().to_string(),
            source.display().to_string(),
        ],
        Some(source.parent().unwrap_or(Path::new("."))),
        &[],
        log,
    )?;
    let delivered = product::deliver_binary(&binary, request, display, log)?;
    Ok(super::finish(
        Language::Rust,
        delivered,
        "rustc",
        vec!["Native executable.".into()],
    ))
}

fn cargo_build(
    request: &BuildRequest,
    _detection: &Detection,
    display: &str,
    manifest: &Path,
    log: &mut dyn Log,
) -> Result<BuildOutcome> {
    let cargo = fsutil::require_tool(&["cargo"], "Install Rust from https://rustup.rs")?;
    let temp = TempDir::new("packager-cargo")?;
    let root = manifest.parent().unwrap_or(Path::new("."));
    let staged = if root.join("Cargo.lock").is_file() {
        None
    } else {
        Some(fsutil::StagedProject::stage(root, &request.output_dir)?)
    };
    let manifest = match &staged {
        Some(stage) => stage.path().join("Cargo.toml"),
        None => manifest.to_path_buf(),
    };
    let cwd = match &staged {
        Some(stage) => stage.path().to_path_buf(),
        None => root.to_path_buf(),
    };
    let names = cargo_bin_names(&cargo, &manifest);
    let mut args = vec![
        "build".into(),
        "--release".into(),
        "--bins".into(),
        "--manifest-path".into(),
        manifest.display().to_string(),
        "--target-dir".into(),
        temp.path().display().to_string(),
    ];
    if manifest
        .parent()
        .is_some_and(|dir| dir.join("Cargo.lock").is_file())
    {
        args.push("--locked".into());
    }
    process::run_ok(&cargo, &args, Some(&cwd), &[], log)?;
    let release = temp.path().join("release");
    let mut binaries = Vec::new();
    if let Some(names) = names {
        for name in names {
            let path = release.join(&name);
            if path.is_file() {
                binaries.push(path);
            }
        }
    }
    if binaries.is_empty() {
        binaries = fsutil::executables_under(&release, 1);
    }
    if binaries.is_empty() {
        return Err(PackagerError::msg(
            "cargo finished without a binary. Add a src/main.rs or a [[bin]] target.",
        ));
    }
    let mut artifacts = Vec::new();
    let mut notes = vec!["Native release build.".to_string()];
    let several = binaries.len() > 1;
    for binary in binaries {
        let name = if several {
            format!("{display} {}", fsutil::file_name(&binary))
        } else {
            display.to_string()
        };
        let delivered = product::deliver_binary(&binary, request, &name, log)?;
        artifacts.extend(delivered.artifacts);
        notes.extend(delivered.notes);
    }
    Ok(super::outcome(Language::Rust, artifacts, "cargo", notes))
}

fn cargo_bin_names(cargo: &Path, manifest: &Path) -> Option<Vec<String>> {
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(manifest)
        .env("PATH", fsutil::enriched_path())
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value: Value = serde_json::from_slice(&output.stdout).ok()?;
    let packages = value.get("packages")?.as_array()?;
    let mut names = Vec::new();
    for package in packages {
        let Some(targets) = package.get("targets").and_then(Value::as_array) else {
            continue;
        };
        for target in targets {
            let is_bin = target
                .get("kind")
                .and_then(Value::as_array)
                .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("bin")));
            if is_bin {
                if let Some(name) = target.get("name").and_then(Value::as_str) {
                    names.push(name.to_string());
                }
            }
        }
    }
    Some(names)
}
