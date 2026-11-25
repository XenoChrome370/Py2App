use std::path::Path;

use crate::error::Result;
use crate::fsutil::{self, TempDir};
use crate::model::{BuildOutcome, BuildRequest, Detection, Language, Toolchain};
use crate::process::{self, Log};
use crate::product;

pub fn toolchain() -> Toolchain {
    match fsutil::find_tool(&["go"]) {
        Some(go) => Toolchain {
            language: "go".into(),
            label: "Go".into(),
            available: true,
            tool: "go".into(),
            detail: go.display().to_string(),
        },
        None => Toolchain {
            language: "go".into(),
            label: "Go".into(),
            available: false,
            tool: "go".into(),
            detail: "Install Go from https://go.dev/dl".into(),
        },
    }
}

pub fn build(
    request: &BuildRequest,
    detection: &Detection,
    display: &str,
    log: &mut dyn Log,
) -> Result<BuildOutcome> {
    let go = fsutil::require_tool(&["go"], "Install Go from https://go.dev/dl")?;
    let temp = TempDir::new("packager-go")?;
    let binary = temp.path().join("app");
    let module = detection.project_root.join("go.mod");
    if module.is_file() {
        let cwd = if detection.entry.is_file() {
            detection
                .entry
                .parent()
                .unwrap_or(detection.project_root.as_path())
        } else {
            detection.project_root.as_path()
        };
        process::run_ok(
            &go,
            &[
                "build".into(),
                "-o".into(),
                binary.display().to_string(),
                ".".into(),
            ],
            Some(cwd),
            &[],
            log,
        )?;
    } else {
        let source = super::source_file(&detection.entry)?;
        process::run_ok(
            &go,
            &[
                "build".into(),
                "-o".into(),
                binary.display().to_string(),
                source.display().to_string(),
            ],
            Some(source.parent().unwrap_or(Path::new("."))),
            &[],
            log,
        )?;
    }
    let artifacts = product::deliver_binary(&binary, request, display, log)?;
    Ok(super::outcome(
        Language::Go,
        artifacts,
        "go",
        vec!["Native executable.".into()],
    ))
}
