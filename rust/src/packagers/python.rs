use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{PackagerError, Result};
use crate::fsutil::{self, TempDir};
use crate::model::{
    bundle_identifier, executable_slug, BuildOutcome, BuildRequest, Detection, Language,
    OutputMode, Toolchain,
};
use crate::process::{self, Log};
use crate::product;

pub fn toolchain() -> Toolchain {
    let python = fsutil::find_tool(&["python3", "python"]);
    let Some(python) = python else {
        return Toolchain {
            language: "python".into(),
            label: "Python".into(),
            available: false,
            tool: "Python".into(),
            detail: "Install Python 3, then python3 -m pip install py2app".into(),
        };
    };
    let py2app = has_module(&python, "py2app");
    let installer = has_module(&python, "PyInstaller");
    let tool = match (py2app, installer) {
        (true, true) => "py2app, PyInstaller",
        (true, false) => "py2app",
        (false, true) => "PyInstaller",
        (false, false) => "Python",
    };
    let detail = if py2app || installer {
        format!("{tool} via {}", python.display())
    } else {
        format!(
            "Python is installed. For an app: {} -m pip install py2app. For a tool: {} -m pip install pyinstaller",
            python.display(),
            python.display()
        )
    };
    Toolchain {
        language: "python".into(),
        label: "Python".into(),
        available: py2app || installer,
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
    let python = fsutil::require_tool(
        &["python3", "python"],
        "Install Python 3 to package Python projects",
    )?;
    let script = python_script(detection)?;
    let root = &detection.project_root;
    let py2app = has_module(&python, "py2app");
    let installer = has_module(&python, "PyInstaller");
    let temp = TempDir::new("packager-python")?;
    let pythonpath = root.display().to_string();

    if request.mode == OutputMode::App && py2app {
        let setup_path = temp.path().join("setup.py");
        fsutil::write_file(
            &setup_path,
            &render_setup(&script, display, request.icon.as_deref(), root)?,
        )?;
        let dist = temp.path().join("dist");
        process::run_ok(
            &python,
            &[
                setup_path.display().to_string(),
                "py2app".into(),
                "--dist-dir".into(),
                dist.display().to_string(),
                "--bdist-base".into(),
                temp.path().join("bdist").display().to_string(),
            ],
            Some(temp.path()),
            &[("PYTHONPATH", pythonpath.as_str())],
            log,
        )?;
        let app = find_new_app(&dist, &std::collections::HashSet::new(), display)?;
        let placed =
            product::place_path(&app, &request.output_dir, &format!("{display}.app"), log)?;
        log.line(
            "py2app wrote the app into the output folder. The project itself was left unchanged.",
        );
        return Ok(super::outcome(
            Language::Python,
            vec![placed],
            "py2app",
            vec!["The app includes a Python runtime.".into()],
        ));
    }

    if installer {
        let dist = temp.path().join("dist");
        let name = if request.mode == OutputMode::App {
            display.to_string()
        } else {
            executable_slug(display)
        };
        let mut args = vec![
            "-m".into(),
            "PyInstaller".into(),
            "--noconfirm".into(),
            "--clean".into(),
            "--name".into(),
            name.clone(),
            "--distpath".into(),
            dist.display().to_string(),
            "--workpath".into(),
            temp.path().join("work").display().to_string(),
            "--specpath".into(),
            temp.path().display().to_string(),
            "--paths".into(),
            root.display().to_string(),
            "--osx-bundle-identifier".into(),
            bundle_identifier(display),
        ];
        if request.mode == OutputMode::App {
            args.push("--windowed".into());
        } else {
            args.push("--onefile".into());
            args.push("--console".into());
        }
        if let Some(icon) = &request.icon {
            args.push("--icon".into());
            args.push(icon.display().to_string());
        }
        args.push(script.display().to_string());
        process::run_ok(&python, &args, Some(root), &[], log)?;
        if request.mode == OutputMode::App {
            let app = dist.join(format!("{name}.app"));
            if !app.is_dir() {
                return Err(PackagerError::msg(format!(
                    "PyInstaller finished without {}",
                    app.display()
                )));
            }
            let dest =
                product::place_path(&app, &request.output_dir, &format!("{display}.app"), log)?;
            return Ok(super::outcome(
                Language::Python,
                vec![dest],
                "PyInstaller",
                vec!["The app includes a Python runtime.".into()],
            ));
        }
        let binary = dist.join(&name);
        let delivered = product::deliver_binary(&binary, request, display, log)?;
        return Ok(super::finish(
            Language::Python,
            delivered,
            "PyInstaller",
            vec!["The tool includes a Python runtime.".into()],
        ));
    }

    let install = if request.mode == OutputMode::App {
        format!("{} -m pip install py2app", python.display())
    } else {
        format!("{} -m pip install pyinstaller", python.display())
    };
    Err(PackagerError::msg(format!(
        "Python packaging tools are not installed. Run {install}"
    )))
}

fn python_script(detection: &Detection) -> Result<PathBuf> {
    if detection.entry.is_file() {
        return Ok(detection.entry.clone());
    }
    Err(PackagerError::msg(
        "choose the Python file that should start the app",
    ))
}

fn has_module(python: &Path, module: &str) -> bool {
    std::process::Command::new(python)
        .args(["-c", &format!("import {module}")])
        .env("PATH", fsutil::enriched_path())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

pub(crate) fn render_setup(
    script: &Path,
    display: &str,
    icon: Option<&Path>,
    root: &Path,
) -> Result<String> {
    let packages = local_packages(root);
    let icon_line = match icon {
        Some(icon) => format!(
            "\n            \"iconfile\": {},",
            py(&icon.display().to_string())
        ),
        None => String::new(),
    };
    Ok(format!(
        r#"from setuptools import setup

setup(
    name={name},
    app=[{script}],
    options={{
        "py2app": {{
            "argv_emulation": False,
            "packages": {packages},{icon}
            "plist": {{
                "CFBundleName": {name},
                "CFBundleDisplayName": {name},
                "CFBundleIdentifier": {bundle},
                "CFBundleShortVersionString": "1.0.0",
                "NSHighResolutionCapable": True,
            }},
        }}
    }},
)
"#,
        name = py(display),
        script = py(&script.display().to_string()),
        packages = serde_json::to_string(&packages).unwrap_or_else(|_| "[]".into()),
        icon = icon_line,
        bundle = py(&bundle_identifier(display)),
    ))
}

fn local_packages(root: &Path) -> Vec<String> {
    let mut names = Vec::new();
    let Ok(read) = fs::read_dir(root) else {
        return names;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() && path.join("__init__.py").is_file() {
            let name = fsutil::file_name(&path);
            if !name.starts_with('.') && name != "build" && name != "dist" {
                names.push(name);
            }
        }
    }
    names.sort();
    names
}

fn py(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".into())
}

fn find_new_app(
    dir: &Path,
    before: &std::collections::HashSet<String>,
    display: &str,
) -> Result<PathBuf> {
    let expected = dir.join(format!("{display}.app"));
    if expected.is_dir() {
        return Ok(expected);
    }
    let mut created = Vec::new();
    if let Ok(read) = fs::read_dir(dir) {
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".app") && !before.contains(&name) {
                created.push(entry.path());
            }
        }
    }
    created
        .pop()
        .ok_or_else(|| PackagerError::msg("py2app finished without writing an .app bundle"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setup_points_at_the_script_and_skips_tool_packages() {
        let script = PathBuf::from("/tmp/demo/main.py");
        let root = PathBuf::from("/tmp/demo");
        let setup = render_setup(&script, "Demo", None, &root).unwrap();
        assert!(setup.contains("/tmp/demo/main.py"));
        assert!(setup.contains("Demo"));
        assert!(!setup.contains("setuptools', 'py2app'"));
    }
}
