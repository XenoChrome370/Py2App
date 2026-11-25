use std::path::Path;

use crate::error::{PackagerError, Result};
use crate::fsutil::{self, TempDir};
use crate::model::{BuildOutcome, BuildRequest, Detection, Language, Toolchain};
use crate::process::{self, Log};
use crate::product;

pub fn toolchain() -> Toolchain {
    match fsutil::find_tool(&["zig"]) {
        Some(zig) => Toolchain {
            language: "zig".into(),
            label: "Zig".into(),
            available: true,
            tool: "zig".into(),
            detail: zig.display().to_string(),
        },
        None => Toolchain {
            language: "zig".into(),
            label: "Zig".into(),
            available: false,
            tool: "zig".into(),
            detail: "Install Zig from https://ziglang.org/download".into(),
        },
    }
}

pub fn build(
    request: &BuildRequest,
    detection: &Detection,
    display: &str,
    log: &mut dyn Log,
) -> Result<BuildOutcome> {
    let zig = fsutil::require_tool(&["zig"], "Install Zig from https://ziglang.org/download")?;
    let temp = TempDir::new("packager-zig")?;
    if detection.project_root.join("build.zig").is_file() {
        let prefix = temp.path().join("prefix");
        process::run_ok(
            &zig,
            &[
                "build".into(),
                "-Doptimize=ReleaseFast".into(),
                "--prefix".into(),
                prefix.display().to_string(),
            ],
            Some(&detection.project_root),
            &[],
            log,
        )?;
        let bins = fsutil::executables_under(&prefix, 3);
        let binary = bins.into_iter().next().ok_or_else(|| {
            PackagerError::msg("zig build finished without an executable in prefix/bin")
        })?;
        let artifacts = product::deliver_binary(&binary, request, display, log)?;
        return Ok(super::outcome(
            Language::Zig,
            artifacts,
            "zig",
            vec!["Native executable.".into()],
        ));
    }
    let source = super::source_file(&detection.entry)?;
    let binary = temp.path().join("app");
    process::run_ok(
        &zig,
        &[
            "build-exe".into(),
            "-O".into(),
            "ReleaseFast".into(),
            format!("-femit-bin={}", binary.display()),
            source.display().to_string(),
        ],
        Some(source.parent().unwrap_or(Path::new("."))),
        &[],
        log,
    )?;
    let artifacts = product::deliver_binary(&binary, request, display, log)?;
    Ok(super::outcome(
        Language::Zig,
        artifacts,
        "zig",
        vec!["Native executable.".into()],
    ))
}
