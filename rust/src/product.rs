use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::{PackagerError, Result};
use crate::fsutil::{self, TempDir};
use crate::model::{bundle_identifier, executable_slug, BuildRequest, OutputMode};
use crate::process::{self, Log};

static UNIQUE_FALLBACK: AtomicU64 = AtomicU64::new(0);

pub struct Delivery {
    pub artifacts: Vec<PathBuf>,
    pub notes: Vec<String>,
}

/// Shell fragment that puts Homebrew, user toolchains, and Python installs
/// ahead of the minimal PATH a Finder-launched app receives.
pub fn shell_path_setup() -> &'static str {
    r#"py_bins=""
for dir in "${HOME:-}"/Library/Python/*/bin /Library/Frameworks/Python.framework/Versions/*/bin; do
  if [ -d "$dir" ]; then
    py_bins="${py_bins:+$py_bins:}$dir"
  fi
done
PATH="${HOME:-}/.cargo/bin:${HOME:-}/go/bin:${HOME:-}/.local/bin:${HOME:-}/.bun/bin:${HOME:-}/.dotnet:${HOME:-}/.dotnet/tools:/opt/homebrew/bin:/usr/local/bin${py_bins:+:$py_bins}:${PATH:-/usr/bin:/bin}"
export PATH"#
}

pub fn deliver_binary(
    binary: &Path,
    request: &BuildRequest,
    display: &str,
    log: &mut dyn Log,
) -> Result<Delivery> {
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
            sign_product(&dest, log);
            Ok(Delivery {
                artifacts: vec![dest],
                notes: Vec::new(),
            })
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
) -> Result<Delivery> {
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
            sign_product(&launcher, log);
            Ok(Delivery {
                artifacts: vec![launcher],
                notes: Vec::new(),
            })
        }
        OutputMode::App => {
            let (app, notes) = prepare_app_bundle(
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
            sign_product(&app, log);
            Ok(Delivery {
                artifacts: vec![app],
                notes,
            })
        }
    }
}

pub fn deliver_tree(
    payload: &Path,
    executable: &Path,
    request: &BuildRequest,
    display: &str,
    log: &mut dyn Log,
) -> Result<Delivery> {
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
            sign_product(&exe, log);
            Ok(Delivery {
                artifacts: vec![exe],
                notes: Vec::new(),
            })
        }
        OutputMode::App => {
            let (app, notes) = prepare_app_bundle(
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
            sign_product(&app, log);
            Ok(Delivery {
                artifacts: vec![app],
                notes,
            })
        }
    }
}

/// Move a finished app or tool to a free name in the output folder.
/// An existing product at that name is left where it is.
pub fn place_path(
    from: &Path,
    output_dir: &Path,
    name: &str,
    log: &mut dyn Log,
) -> Result<PathBuf> {
    fs::create_dir_all(output_dir)?;
    let dest = unique_path(&output_dir.join(name));
    if from.is_dir() {
        fsutil::copy_tree(from, &dest)?;
    } else {
        fsutil::copy_file(from, &dest)?;
        if fsutil::is_executable(from) {
            fsutil::make_executable(&dest)?;
        }
    }
    sign_product(&dest, log);
    Ok(dest)
}

fn wrap_binary(
    binary: &Path,
    request: &BuildRequest,
    display: &str,
    slug: &str,
    log: &mut dyn Log,
) -> Result<Delivery> {
    let (app, notes) = prepare_app_bundle(
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
    sign_product(&app, log);
    Ok(Delivery {
        artifacts: vec![app],
        notes,
    })
}

fn prepare_app_bundle(
    output_dir: &Path,
    display: &str,
    slug: &str,
    icon: Option<&Path>,
    log: &mut dyn Log,
) -> Result<(PathBuf, Vec<String>)> {
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
        let macos = app.join("Contents/MacOS");
        let resources = app.join("Contents/Resources");
        fs::create_dir_all(&macos)?;
        fs::create_dir_all(&resources)?;
        let notes = install_icon(&resources, icon, log)?;
        let has_icon = resources.join("AppIcon.icns").is_file();
        let plist = render_plist(display, slug, &bundle_identifier(display), has_icon);
        fsutil::write_file(&app.join("Contents/Info.plist"), &plist)?;
        fsutil::write_file(&app.join("Contents/PkgInfo"), "APPL????")?;
        Ok((app, notes))
    }
}

fn launcher_script(interpreter: Option<&str>, root_expr: &str, relative: &str) -> String {
    let quoted = fsutil::bash_single(relative);
    let exec_line = match interpreter {
        Some(program) => format!("exec /usr/bin/env {program} \"$ROOT\"/{quoted} \"$@\""),
        None => format!("exec \"$ROOT\"/{quoted} \"$@\""),
    };
    let path_setup = shell_path_setup();
    format!(
        "#!/bin/bash\nset -euo pipefail\n{path_setup}\nHERE=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\nROOT=\"$(cd \"$HERE/{root_expr}\" && pwd)\"\ncd \"$ROOT\"\n{exec_line}\n"
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

fn install_icon(resources: &Path, icon: Option<&Path>, log: &mut dyn Log) -> Result<Vec<String>> {
    let Some(icon) = icon else {
        return Ok(Vec::new());
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
            Ok(Vec::new())
        }
        Some("png") | Some("jpg") | Some("jpeg") | Some("tiff") | Some("tif") => {
            log.line(&format!("converting icon {}", fsutil::file_name(icon)));
            match raster_to_icns(icon, &dest) {
                Ok(()) => Ok(Vec::new()),
                Err(err) => Ok(vec![format!("icon skipped: {err}")]),
            }
        }
        _ => Ok(vec!["icon skipped: use a .icns or .png file".to_string()]),
    }
}

fn raster_to_icns(src: &Path, dest: &Path) -> Result<()> {
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
        process::run_silent(
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
        )?;
    }
    process::run_silent(
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
    let n = UNIQUE_FALLBACK.fetch_add(1, Ordering::Relaxed);
    parent.join(format!("{stem}-{}-{n}{ext}", std::process::id()))
}

fn sign_product(path: &Path, log: &mut dyn Log) {
    #[cfg(target_os = "macos")]
    {
        let Some(codesign) = fsutil::find_tool(&["codesign"]) else {
            log.line("code signing skipped: codesign is not available");
            return;
        };
        if path.extension().and_then(|ext| ext.to_str()) == Some("app") {
            let macos = path.join("Contents/MacOS");
            if let Ok(read) = fs::read_dir(&macos) {
                for entry in read.flatten() {
                    let exe = entry.path();
                    if exe.is_file() {
                        sign_file(&codesign, &exe, log);
                    }
                }
            }
            for exe in fsutil::executables_under(&path.join("Contents/Resources/payload"), 4) {
                sign_file(&codesign, &exe, log);
            }
        }
        sign_file(&codesign, path, log);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (path, log);
    }
}

#[cfg(target_os = "macos")]
fn sign_file(codesign: &Path, path: &Path, log: &mut dyn Log) {
    let args = vec![
        "--force".to_string(),
        "--sign".to_string(),
        "-".to_string(),
        path.display().to_string(),
    ];
    if let Err(err) = process::run_silent(codesign, &args, None, &[]) {
        log.line(&format!("code signing skipped: {err}"));
    }
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
        let delivered = deliver_script(&script, None, &request, "Hello", &mut log).unwrap();
        let app = &delivered.artifacts[0];
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
        let launcher = fs::read_to_string(&exe).unwrap();
        assert!(launcher.contains("/opt/homebrew/bin"));
        assert!(launcher.contains("Library/Python"));
    }

    #[test]
    fn existing_app_is_left_in_place() {
        let root = crate::fsutil::TempDir::new("packager-keep-app").unwrap();
        let output = root.path().join("out");
        fs::create_dir_all(&output).unwrap();
        let existing = output.join("Hello.app");
        fs::create_dir_all(existing.join("Contents")).unwrap();
        fs::write(existing.join("Contents/marker"), "keep").unwrap();
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
        let delivered = deliver_script(&script, None, &request, "Hello", &mut log).unwrap();
        assert_ne!(delivered.artifacts[0], existing);
        assert_eq!(
            fs::read_to_string(existing.join("Contents/marker")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn plist_escapes_markup() {
        let plist = render_plist("A & B <C>", "tool", "dev.packager.a", false);
        assert!(plist.contains("A &amp; B &lt;C&gt;"));
        assert!(!plist.contains("A & B"));
    }
}
