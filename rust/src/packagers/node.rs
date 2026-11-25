use crate::error::{PackagerError, Result};
use crate::fsutil::{self, TempDir};
use crate::model::{executable_slug, BuildOutcome, BuildRequest, Detection, Language, Toolchain};
use crate::process::{self, Log};
use crate::product;

pub fn toolchain(language: Language) -> Toolchain {
    let bun = fsutil::find_tool(&["bun"]);
    let node = fsutil::find_tool(&["node"]);
    let pkg = fsutil::find_tool(&["pkg"]);
    let (available, tool, detail) = match language {
        Language::TypeScript => {
            if let Some(bun) = bun {
                (
                    true,
                    "bun",
                    format!("compiles TypeScript with {}", bun.display()),
                )
            } else {
                (
                    false,
                    "bun",
                    "Install bun to compile TypeScript into an application".into(),
                )
            }
        }
        _ => {
            if let Some(bun) = &bun {
                (
                    true,
                    "bun",
                    format!("self-contained builds with {}", bun.display()),
                )
            } else if let Some(pkg) = &pkg {
                (
                    true,
                    "pkg",
                    format!("self-contained builds with {}", pkg.display()),
                )
            } else if let Some(node) = &node {
                (true, "node", format!("launcher uses {}", node.display()))
            } else {
                (
                    false,
                    "node",
                    "Install Node.js, bun, or pkg to package JavaScript".into(),
                )
            }
        }
    };
    Toolchain {
        language: language.id().into(),
        label: language.label().into(),
        available,
        tool: tool.into(),
        detail,
    }
}

pub fn build(
    request: &BuildRequest,
    detection: &Detection,
    display: &str,
    log: &mut dyn Log,
) -> Result<BuildOutcome> {
    let entry = super::source_file(&detection.entry)?;
    let typescript = detection.language == Language::TypeScript
        || matches!(fsutil::extension_of(&entry).as_deref(), Some("ts" | "tsx"));
    let temp = TempDir::new("packager-node")?;
    let compiled = temp.path().join("app");

    if let Some(bun) = fsutil::find_tool(&["bun"]) {
        process::run_ok(
            &bun,
            &[
                "build".into(),
                entry.display().to_string(),
                "--compile".into(),
                "--outfile".into(),
                compiled.display().to_string(),
            ],
            Some(&detection.project_root),
            &[],
            log,
        )?;
        let artifacts = product::deliver_binary(&compiled, request, display, log)?;
        return Ok(super::outcome(
            detection.language,
            artifacts,
            "bun",
            vec!["Self-contained executable built with bun.".into()],
        ));
    }

    if typescript {
        return Err(PackagerError::msg(
            "TypeScript needs bun (bun build --compile). Install bun, or package a JavaScript file.",
        ));
    }

    if let Some(pkg) = fsutil::find_tool(&["pkg"]) {
        process::run_ok(
            &pkg,
            &[
                entry.display().to_string(),
                "--output".into(),
                compiled.display().to_string(),
            ],
            Some(&detection.project_root),
            &[],
            log,
        )?;
        let artifacts = product::deliver_binary(&compiled, request, display, log)?;
        return Ok(super::outcome(
            detection.language,
            artifacts,
            "pkg",
            vec!["Self-contained executable built with pkg.".into()],
        ));
    }

    let node = fsutil::require_tool(&["node"], "Install Node.js or bun to package JavaScript")?;
    let _ = node;
    if detection.project_root.join("package.json").is_file() && detection.entry.is_file() {
        let payload = temp.path().join("project");
        fsutil::copy_tree_excluding(
            &detection.project_root,
            &payload,
            &[request.output_dir.clone()],
        )?;
        let relative = entry
            .strip_prefix(&detection.project_root)
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| fsutil::file_name(&entry));
        let slug = executable_slug(display);
        let launcher_name = if payload.join(&slug).exists() {
            "packager-launch".to_string()
        } else {
            slug
        };
        let launcher = payload.join(&launcher_name);
        fsutil::write_file(&launcher, &node_launcher(&relative))?;
        fsutil::make_executable(&launcher)?;
        let artifacts = product::deliver_tree(&payload, &launcher, request, display, log)?;
        return Ok(super::outcome(
            detection.language,
            artifacts,
            "node",
            vec!["The app runs with Node.js on the machine that opens it.".into()],
        ));
    }

    let artifacts = product::deliver_script(&entry, Some("node"), request, display, log)?;
    Ok(super::outcome(
        detection.language,
        artifacts,
        "node",
        vec!["The app runs with Node.js on the machine that opens it.".into()],
    ))
}

fn node_launcher(relative: &str) -> String {
    let quoted = fsutil::bash_single(relative);
    format!(
        "#!/bin/bash\nset -euo pipefail\nHERE=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\ncd \"$HERE\"\nexec /usr/bin/env node \"$HERE\"/{quoted} \"$@\"\n"
    )
}
