use std::path::Path;

use crate::error::{PackagerError, Result};
use crate::fsutil::{self, TempDir};
use crate::model::{BuildOutcome, BuildRequest, Detection, Language, Toolchain};
use crate::process::{self, Log};
use crate::product;

pub fn toolchain() -> Toolchain {
    match fsutil::find_tool(&["dart"]) {
        Some(dart) => Toolchain {
            language: "dart".into(),
            label: "Dart".into(),
            available: true,
            tool: "dart".into(),
            detail: dart.display().to_string(),
        },
        None => Toolchain {
            language: "dart".into(),
            label: "Dart".into(),
            available: false,
            tool: "dart".into(),
            detail: "Install the Dart SDK to compile Dart programs".into(),
        },
    }
}

pub fn build(
    request: &BuildRequest,
    detection: &Detection,
    display: &str,
    log: &mut dyn Log,
) -> Result<BuildOutcome> {
    let pubspec = detection.project_root.join("pubspec.yaml");
    if pubspec.is_file() {
        let text = fsutil::read_to_string(&pubspec).unwrap_or_default();
        if text.contains("sdk: flutter") {
            return Err(PackagerError::msg(
                "Flutter projects are built with flutter build macos.",
            ));
        }
    }
    let dart = fsutil::require_tool(&["dart"], "Install the Dart SDK to compile Dart programs")?;
    let source = super::source_file(&detection.entry)?;
    let temp = TempDir::new("packager-dart")?;
    let binary = temp.path().join("app");
    process::run_ok(
        &dart,
        &[
            "compile".into(),
            "exe".into(),
            "-o".into(),
            binary.display().to_string(),
            source.display().to_string(),
        ],
        Some(source.parent().unwrap_or(Path::new("."))),
        &[],
        log,
    )?;
    let artifacts = product::deliver_binary(&binary, request, display, log)?;
    Ok(super::outcome(
        Language::Dart,
        artifacts,
        "dart",
        vec!["Self-contained Dart executable.".into()],
    ))
}
