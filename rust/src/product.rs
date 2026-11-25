use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{PackagerError, Result};
use crate::fsutil::{self, TempDir};
use crate::model::{bundle_identifier, executable_slug, BuildRequest, OutputMode};
use crate::process::{self, Log};

pub fn deliver_binary(
    binary: &Path,
    request: &BuildRequest,
    display: &str,
    log: &mut dyn Log,
) -> Result<Vec<PathBuf>> {
    if !binary.is_file() {
        return Err(PackagerError::msg(format!(
            "build finished without an executable at {}",
            binary.display()
        )));
    }
    fs::create_dir_all(&request.output_dir)?;
    let slug = executable_slug(display);
    match request.mode {
        OutputMode::Tool => {
            let dest = unique_path(&request.output_dir.join(&slug));
            fsutil::copy_file(binary, &dest)?;
            fsutil::make_executable(&dest)?;
            log.line(&format!("tool {}", dest.display()));
            Ok(vec![dest])
        }
        OutputMode::App => wrap_binary(binary, request, display, &slug, log),
    }
}

pub fn deliver_script(
    source: &Path,
    interpreter: Option<&str>,
    request: &BuildRequest,
    display: &str,
    log: &mut dyn Log,
) -> Result<Vec<PathBuf>> {
    let staging = TempDir::new("packager-script")?;
    let file_name = fsutil::file_name(source);
    let staged = staging.path().join(&file_name);
    fsutil::copy_file(source, &staged)?;
    fsutil::make_executable(&staged)?;
    fs::create_dir_all(&request.output_dir)?;
    let slug = executable_slug(display);
    match request.mode {
        OutputMode::Tool => {
            let folder = unique_path(&request.output_dir.join(&slug));
            fs::create_dir_all(&folder)?;
            let payload = folder.join(&file_name);
            fsutil::copy_file(&staged, &payload)?;
            fsutil::make_executable(&payload)?;
            let launcher = folder.join(&slug);
            fsutil::write_file(&launcher, &launcher_script(interpreter, ".", &file_name))?;
            fsutil::make_executable(&launcher)?;
            log.line(&format!("tool {}", launcher.display()));
            Ok(vec![launcher])
        }
        OutputMode::App => {
            let app = prepare_app_bundle(
                &request.output_dir,
                display,
                &slug,
                request.icon.as_deref(),
                log,
            )?;
            let payload_dir = app.join("Contents/Resources/payload");
            fs::create_dir_all(&payload_dir)?;
            let payload = payload_dir.join(&file_name);
            fsutil::copy_file(&staged, &payload)?;
            fsutil::make_executable(&payload)?;
            let launcher = app.join("Contents/MacOS").join(&slug);
            fsutil::write_file(
                &launcher,
                &launcher_script(interpreter, "../Resources/payload", &file_name),
            )?;
            fsutil::make_executable(&launcher)?;
            log.line(&format!("app {}", app.display()));
            Ok(vec![app])
        }
    }
}

pub fn deliver_tree(
    payload: &Path,
    executable: &Path,
    request: &BuildRequest,
    display: &str,
    log: &mut dyn Log,
) -> Result<Vec<PathBuf>> {
    if !payload.is_dir() {
        return Err(PackagerError::msg(format!(
            "missing build output {}",
            payload.display()
        )));
    }
    let relative = executable.strip_prefix(payload).unwrap_or(executable);
    fs::create_dir_all(&request.output_dir)?;
    let slug = executable_slug(display);
    match request.mode {
        OutputMode::Tool => {
            let folder = unique_path(&request.output_dir.join(&slug));
            fsutil::copy_tree(payload, &folder)?;
            let exe = folder.join(relative);
            if exe.is_file() {
                fsutil::make_executable(&exe)?;
            }
            log.line(&format!("tool {}", exe.display()));
            Ok(vec![exe])
        }
        OutputMode::App => {
            let app = prepare_app_bundle(
                &request.output_dir,
                display,
                &slug,
                request.icon.as_deref(),
                log,
            )?;
            let dest_payload = app.join("Contents/Resources/payload");
            fsutil::copy_tree(payload, &dest_payload)?;
            let exe = dest_payload.join(relative);
            if exe.is_file() {
                fsutil::make_executable(&exe)?;
            }
            let relative_text = relative.to_string_lossy().replace('\\', "/");
            let launcher = app.join("Contents/MacOS").join(&slug);
            fsutil::write_file(
                &launcher,
                &launcher_script(None, "../Resources/payload", &relative_text),
            )?;
            fsutil::make_executable(&launcher)?;
            log.line(&format!("app {}", app.display()));
            Ok(vec![app])
        }
    }
}

fn wrap_binary(
    binary: &Path,
    request: &BuildRequest,
    display: &str,
    slug: &str,
    log: &mut dyn Log,
) -> Result<Vec<PathBuf>> {
    let app = prepare_app_bundle(
        &request.output_dir,
        display,
        slug,
        request.icon.as_deref(),
        log,
    )?;
    let dest = app.join("Contents/MacOS").join(slug);
    fsutil::copy_file(binary, &dest)?;
    fsutil::make_executable(&dest)?;
    log.line(&format!("app {}", app.display()));
    Ok(vec![app])
}

fn prepare_app_bundle(
    output_dir: &Path,
    display: &str,
    slug: &str,
    icon: Option<&Path>,
    log: &mut dyn Log,
) -> Result<PathBuf> {
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (output_dir, display, slug, icon, log);
        return Err(PackagerError::msg(
            "App mode produces a macOS .app bundle. Use --mode tool on this system.".to_string(),
        ));
    }
    #[cfg(target_os = "macos")]
    {
        let app = unique_path(&output_dir.join(format!("{display}.app")));
        if app.exists() {
            fs::remove_dir_all(&app)?;
        }
        let macos = app.join("Contents/MacOS");
        let resources = app.join("Contents/Resources");
        fs::create_dir_all(&macos)?;
        fs::create_dir_all(&resources)?;
        let has_icon = install_icon(&resources, icon, log)?;
        let plist = render_plist(display, slug, &bundle_identifier(display), has_icon);
        fsutil::write_file(&app.join("Contents/Info.plist"), &plist)?;
        fsutil::write_file(&app.join("Contents/PkgInfo"), "APPL????")?;
        Ok(app)
    }
}

fn launcher_script(interpreter: Option<&str>, root_expr: &str, relative: &str) -> String {
    let quoted = fsutil::bash_single(relative);
    let exec_line = match interpreter {
        Some(program) => format!("exec /usr/bin/env {program} \"$ROOT\"/{quoted} \"$@\""),
        None => format!("exec \"$ROOT\"/{quoted} \"$@\""),
    };
    format!(
        "#!/bin/bash\nset -euo pipefail\nHERE=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\nROOT=\"$(cd \"$HERE/{root_expr}\" && pwd)\"\ncd \"$ROOT\"\n{exec_line}\n"
    )
}

fn render_plist(display: &str, executable: &str, bundle_id: &str, icon: bool) -> String {
    let icon_xml = if icon {
        "  <key>CFBundleIconFile</key>\n  <string>AppIcon</string>\n"
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>{name}</string>
  <key>CFBundleDisplayName</key>
  <string>{name}</string>
  <key>CFBundleIdentifier</key>
  <string>{bundle_id}</string>
  <key>CFBundleVersion</key>
  <string>1.0.0</string>
  <key>CFBundleShortVersionString</key>
  <string>1.0.0</string>
  <key>CFBundleExecutable</key>
  <string>{executable}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>LSMinimumSystemVersion</key>
  <string>11.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
{icon_xml}</dict>
</plist>
"#,
        name = xml_escape(display),
        bundle_id = xml_escape(bundle_id),
        executable = xml_escape(executable),
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn install_icon(resources: &Path, icon: Option<&Path>, log: &mut dyn Log) -> Result<bool> {
    let Some(icon) = icon else {
        return Ok(false);
    };
    if !icon.is_file() {
        return Err(PackagerError::msg(format!(
            "icon not found: {}",
            icon.display()
        )));
    }
    let dest = resources.join("AppIcon.icns");
    match fsutil::extension_of(icon).as_deref() {
        Some("icns") => {
            fsutil::copy_file(icon, &dest)?;
            Ok(true)
        }
        Some("png") | Some("jpg") | Some("jpeg") | Some("tiff") | Some("tif") => {
            match raster_to_icns(icon, &dest, log) {
                Ok(()) => Ok(true),
                Err(err) => {
                    log.line(&format!("icon skipped: {err}"));
                    Ok(false)
                }
            }
        }
        _ => {
            log.line("icon skipped: use a .icns or .png file");
            Ok(false)
        }
    }
}

fn raster_to_icns(src: &Path, dest: &Path, log: &mut dyn Log) -> Result<()> {
    let sips = fsutil::require_tool(&["sips"], "sips is required to convert an icon")?;
    let iconutil = fsutil::require_tool(&["iconutil"], "iconutil is required to convert an icon")?;
    let temp = TempDir::new("packager-icon")?;
    let iconset = temp.path().join("AppIcon.iconset");
    fs::create_dir_all(&iconset)?;
    for (pixels, name) in [
        (16, "icon_16x16.png"),
        (32, "icon_16x16@2x.png"),
        (32, "icon_32x32.png"),
        (64, "icon_32x32@2x.png"),
        (128, "icon_128x128.png"),
        (256, "icon_128x128@2x.png"),
        (256, "icon_256x256.png"),
        (512, "icon_256x256@2x.png"),
        (512, "icon_512x512.png"),
        (1024, "icon_512x512@2x.png"),
    ] {
        process::run_ok(
            &sips,
            &[
                "-z".to_string(),
                pixels.to_string(),
                pixels.to_string(),
                src.display().to_string(),
                "--out".to_string(),
                iconset.join(name).display().to_string(),
            ],
            None,
            &[],
            log,
        )?;
    }
    process::run_ok(
        &iconutil,
        &[
            "-c".to_string(),
            "icns".to_string(),
            iconset.display().to_string(),
            "-o".to_string(),
            dest.display().to_string(),
        ],
        None,
        &[],
        log,
    )?;
    Ok(())
}

fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    let (stem, ext) = match file_name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), format!(".{ext}")),
        _ => (file_name.to_string(), String::new()),
    };
    for index in 2..1000 {
        let candidate = parent.join(format!("{stem}-{index}{ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    parent.join(format!("{stem}-built{ext}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::OutputMode;
    use crate::process::Log;

    struct CaptureLog;
    impl Log for CaptureLog {
        fn line(&mut self, _text: &str) {}
    }

    #[test]
    fn app_bundle_runs_a_shell_payload() {
        let root = crate::fsutil::TempDir::new("packager-product-test").unwrap();
        let output = root.path().join("out");
        fs::create_dir_all(&output).unwrap();
        let script = root.path().join("hello.sh");
        fsutil::write_file(&script, "#!/bin/bash\necho hello-packager\n").unwrap();
        let request = BuildRequest {
            source: script.clone(),
            output_dir: output,
            language: None,
            name: Some("Hello".into()),
            mode: OutputMode::App,
            icon: None,
        };
        let mut log = CaptureLog;
        let artifacts = deliver_script(&script, None, &request, "Hello", &mut log).unwrap();
        let app = &artifacts[0];
        assert_eq!(app.extension().and_then(|ext| ext.to_str()), Some("app"));
        let plist = fs::read_to_string(app.join("Contents/Info.plist")).unwrap();
        assert!(plist.contains("<string>hello</string>"));
        let exe = app.join("Contents/MacOS/hello");
        let ran = std::process::Command::new(&exe).output().unwrap();
        assert!(
            ran.status.success(),
            "{}",
            String::from_utf8_lossy(&ran.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&ran.stdout), "hello-packager\n");
    }
}
