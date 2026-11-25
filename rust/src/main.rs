use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use packager::{
    active_child_pgid, BuildOutcome, BuildRequest, Language, Log, OutputMode, PackagerError,
    StdoutLog,
};
use serde_json::json;

fn main() -> ExitCode {
    install_signals();
    match dispatch() {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            println!("{err}");
            println!(
                "@@PACKAGER@@ {}",
                serde_json::to_string(&json!({"ok": false, "error": err.message()}))
                    .unwrap_or_default()
            );
            ExitCode::from(1)
        }
    }
}

fn dispatch() -> packager::Result<u8> {
    let mut args = env::args().skip(1);
    let command = args.next();
    match command.as_deref() {
        None | Some("--help") | Some("-h") | Some("help") => {
            print_help();
            Ok(0)
        }
        Some("--version") | Some("-V") => {
            println!("packager {VERSION}");
            Ok(0)
        }
        Some("languages") => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({"languages": packager::languages()}))
                    .unwrap_or_else(|_| "{}".into())
            );
            Ok(0)
        }
        Some("toolchains") => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({"tools": packager::toolchains()}))
                    .unwrap_or_else(|_| "{}".into())
            );
            Ok(0)
        }
        Some("detect") => detect_command(args),
        Some("build") => build_command(args),
        Some(other) => Err(PackagerError::msg(format!(
            "unknown command \"{other}\". Run packager --help"
        ))),
    }
}

fn detect_command(args: impl Iterator<Item = String>) -> packager::Result<u8> {
    let mut path = None;
    let mut language = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--language" => {
                let value = required(&mut args, "--language")?;
                if value != "auto" {
                    language = Some(parse_language(&value)?);
                }
            }
            other if other.starts_with('-') => {
                return Err(PackagerError::msg(format!("unknown option {other}")));
            }
            other if path.is_none() => path = Some(PathBuf::from(other)),
            other => return Err(PackagerError::msg(format!("unexpected argument {other}"))),
        }
    }
    let path =
        path.ok_or_else(|| PackagerError::msg("pass the project or source file to detect"))?;
    match packager::resolve(&path, language) {
        Ok(detection) => {
            let body = json!({
                "ok": true,
                "language": detection.language.id(),
                "label": detection.language.label(),
                "name": packager::suggested_name(&detection),
                "reason": detection.reason,
                "entry": detection.entry.display().to_string(),
                "project_root": detection.project_root.display().to_string(),
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&body).unwrap_or_else(|_| "{}".into())
            );
            Ok(0)
        }
        Err(err) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({"ok": false, "error": err.message()}))
                    .unwrap_or_default()
            );
            Ok(1)
        }
    }
}

fn build_command(args: impl Iterator<Item = String>) -> packager::Result<u8> {
    let mut source = None;
    let mut output = None;
    let mut language = None;
    let mut name = None;
    let mut mode = OutputMode::App;
    let mut icon = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--source" => source = Some(PathBuf::from(required(&mut args, "--source")?)),
            "--output" => output = Some(PathBuf::from(required(&mut args, "--output")?)),
            "--name" => name = Some(required(&mut args, "--name")?),
            "--icon" => icon = Some(PathBuf::from(required(&mut args, "--icon")?)),
            "--language" => {
                let value = required(&mut args, "--language")?;
                if value != "auto" {
                    language = Some(parse_language(&value)?);
                }
            }
            "--mode" => {
                let value = required(&mut args, "--mode")?;
                mode = OutputMode::parse(&value)
                    .ok_or_else(|| PackagerError::msg("mode must be app or tool"))?;
            }
            other => {
                return Err(PackagerError::msg(format!(
                    "unknown option {other}. Run packager build --help"
                )))
            }
        }
    }
    let request = BuildRequest {
        source: source.ok_or_else(|| PackagerError::msg("pass --source"))?,
        output_dir: output.ok_or_else(|| PackagerError::msg("pass --output"))?,
        language,
        name,
        mode,
        icon,
    };
    let mut log = StdoutLog;
    match packager::build(&request, &mut log) {
        Ok(outcome) => {
            emit_result(true, Some(&outcome), None);
            Ok(0)
        }
        Err(err) => {
            log.line(err.message());
            emit_result(false, None, Some(err.message()));
            Ok(1)
        }
    }
}

fn emit_result(ok: bool, outcome: Option<&BuildOutcome>, error: Option<&str>) {
    let body = json!({
        "ok": ok,
        "language": outcome.map(|item| item.language.id()),
        "label": outcome.map(|item| item.language.label()),
        "artifacts": outcome
            .map(|item| item.artifacts.iter().map(|path| path.display().to_string()).collect::<Vec<_>>())
            .unwrap_or_default(),
        "tool": outcome.map(|item| item.tool.as_str()).unwrap_or(""),
        "notes": outcome.map(|item| item.notes.clone()).unwrap_or_default(),
        "error": error,
    });
    println!(
        "@@PACKAGER@@ {}",
        serde_json::to_string(&body).unwrap_or_else(|_| "{\"ok\":false}".into())
    );
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

fn required(
    args: &mut std::iter::Peekable<impl Iterator<Item = String>>,
    flag: &str,
) -> packager::Result<String> {
    args.next()
        .filter(|value| !value.starts_with('-'))
        .ok_or_else(|| PackagerError::msg(format!("{flag} needs a value")))
}

fn parse_language(value: &str) -> packager::Result<Language> {
    Language::parse(value).ok_or_else(|| {
        PackagerError::msg(format!(
            "unknown language \"{value}\". Run packager languages"
        ))
    })
}

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn print_help() {
    println!(
        "packager {VERSION}
Package a project into a macOS app or a command-line tool.

Usage:
  packager languages
  packager toolchains
  packager detect <path> [--language <id>]
  packager build --source <path> --output <dir> [options]

Options:
  --language <id>   Language id, or auto
  --name <name>     Application name
  --mode app|tool   app is the default
  --icon <file>     .icns or .png icon

The Swift app and this command package projects on the computer they run on."
    );
}

fn install_signals() {
    unsafe {
        signal(2, handle_signal);
        signal(13, ignore_signal);
        signal(15, handle_signal);
    }
}

unsafe extern "C" fn ignore_signal(_signal_number: i32) {}

unsafe extern "C" fn handle_signal(signal_number: i32) {
    let pgid = active_child_pgid();
    if pgid > 0 {
        kill(-pgid, 15);
    }
    _exit(128 + signal_number);
}

unsafe extern "C" {
    fn signal(sig: i32, handler: unsafe extern "C" fn(i32)) -> usize;
    fn kill(pid: i32, sig: i32) -> i32;
    fn _exit(code: i32) -> !;
}
