use std::io::{BufRead, BufReader, Read};
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::mpsc;
use std::thread;

use crate::error::{PackagerError, Result};
use crate::fsutil::{enriched_path, quote_arg};

static CHILD_PGID: AtomicI32 = AtomicI32::new(0);

pub fn active_child_pgid() -> i32 {
    CHILD_PGID.load(Ordering::SeqCst)
}

pub trait Log {
    fn line(&mut self, text: &str);
}

pub struct StdoutLog;

impl Log for StdoutLog {
    fn line(&mut self, text: &str) {
        println!("{text}");
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }
}

pub fn run(
    program: &Path,
    args: &[String],
    cwd: Option<&Path>,
    extra_env: &[(&str, &str)],
    log: &mut dyn Log,
) -> Result<i32> {
    run_command(program, args, cwd, extra_env, true, log)
}

/// Run a command without echoing it. Output is returned only when it fails,
/// so icon conversion and code signing stay off the build log unless they break.
pub fn run_silent(
    program: &Path,
    args: &[String],
    cwd: Option<&Path>,
    extra_env: &[(&str, &str)],
) -> Result<()> {
    let mut captured = CaptureLog::default();
    let code = run_command(program, args, cwd, extra_env, false, &mut captured)?;
    if code != 0 {
        let detail = captured.lines.join("\n");
        let suffix = if detail.is_empty() {
            String::new()
        } else {
            format!("\n{detail}")
        };
        return Err(PackagerError::msg(format!(
            "{} exited with status {code}{suffix}",
            program.display()
        )));
    }
    Ok(())
}

#[derive(Default)]
struct CaptureLog {
    lines: Vec<String>,
}

impl Log for CaptureLog {
    fn line(&mut self, text: &str) {
        self.lines.push(text.to_string());
    }
}

fn run_command(
    program: &Path,
    args: &[String],
    cwd: Option<&Path>,
    extra_env: &[(&str, &str)],
    echo: bool,
    log: &mut dyn Log,
) -> Result<i32> {
    if echo {
        let mut rendered = quote_arg(&program.display().to_string());
        for arg in args {
            rendered.push(' ');
            rendered.push_str(&quote_arg(arg));
        }
        log.line(&format!("$ {rendered}"));
    }

    let mut command = Command::new(program);
    command
        .args(args)
        .env("PATH", enriched_path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    for (key, value) in extra_env {
        command.env(key, value);
    }
    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            if setpgid(0, 0) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let mut child = command.spawn().map_err(|err| {
        PackagerError::msg(format!("could not start {}: {err}", program.display()))
    })?;
    let pid = child.id() as i32;
    CHILD_PGID.store(pid, Ordering::SeqCst);

    let stdout = child.stdout.take().expect("stdout pipe");
    let stderr = child.stderr.take().expect("stderr pipe");
    let (tx, rx) = mpsc::channel::<String>();
    let tx_err = tx.clone();
    thread::spawn(move || pump(stdout, tx));
    thread::spawn(move || pump(stderr, tx_err));
    for line in rx {
        log.line(&line);
    }

    let status = child.wait().map_err(|err| {
        CHILD_PGID.store(0, Ordering::SeqCst);
        PackagerError::msg(format!("{err}"))
    })?;
    CHILD_PGID.store(0, Ordering::SeqCst);
    Ok(status.code().unwrap_or(1))
}

pub fn run_ok(
    program: &Path,
    args: &[String],
    cwd: Option<&Path>,
    extra_env: &[(&str, &str)],
    log: &mut dyn Log,
) -> Result<()> {
    let code = run(program, args, cwd, extra_env, log)?;
    if code != 0 {
        return Err(PackagerError::msg(format!(
            "{} exited with status {code}",
            program.display()
        )));
    }
    Ok(())
}

pub fn output_text(program: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new(program)
        .args(args)
        .env("PATH", enriched_path())
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|text| text.trim().to_string())
}

fn pump<R: Read>(reader: R, tx: mpsc::Sender<String>) {
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if line.ends_with('\n') {
                    line.pop();
                    if line.ends_with('\r') {
                        line.pop();
                    }
                }
                if tx.send(line.clone()).is_err() {
                    break;
                }
            }
        }
    }
}

#[cfg(unix)]
unsafe extern "C" {
    fn setpgid(pid: i32, pgid: i32) -> i32;
}
