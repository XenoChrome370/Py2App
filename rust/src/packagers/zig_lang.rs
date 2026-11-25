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
    let cache = temp.path().join("cache");
    if detection.project_root.join("build.zig").is_file() {
        let staged = fsutil::StagedProject::stage(&detection.project_root, &request.output_dir)?;
        let prefix = temp.path().join("prefix");
        process::run_ok(
            &zig,
            &[
                "build".into(),
                "-Doptimize=ReleaseFast".into(),
                "--prefix".into(),
                prefix.display().to_string(),
                "--cache-dir".into(),
                cache.display().to_string(),
            ],
            Some(staged.path()),
            &[],
            log,
        )?;
        let bins = fsutil::executables_under(&prefix, 3);
        let binary = bins.into_iter().next().ok_or_else(|| {
            PackagerError::msg("zig build finished without an executable in prefix/bin")
        })?;
        let delivered = product::deliver_binary(&binary, request, display, log)?;
        return Ok(super::finish(
            Language::Zig,
            delivered,
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
            "--cache-dir".into(),
            cache.display().to_string(),
            "-O".into(),
            "ReleaseFast".into(),
            format!("-femit-bin={}", binary.display()),
            source.display().to_string(),
        ],
        Some(source.parent().unwrap_or(Path::new("."))),
        &[],
        log,
    )?;
    let delivered = product::deliver_binary(&binary, request, display, log)?;
    Ok(super::finish(
        Language::Zig,
        delivered,
        "zig",
        vec!["Native executable.".into()],
    ))
}
