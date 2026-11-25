use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use packager::{build, BuildRequest, Language, OutputMode, PackagerError};

static SEQ: AtomicU64 = AtomicU64::new(1);

struct Capture {
    lines: Vec<String>,
}

impl packager::Log for Capture {
    fn line(&mut self, text: &str) {
        self.lines.push(text.to_string());
    }
}

fn workspace(name: &str) -> PathBuf {
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.push(".tmp");
    path.push(format!("{name}-{seq}"));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

fn tool_installed(name: &str) -> bool {
    match Command::new(name).arg("--version").output() {
        Ok(_) => true,
        Err(_) => {
            eprintln!("skip: {name} is not installed");
            false
        }
    }
}

fn request(source: &Path, output: &Path, mode: OutputMode) -> BuildRequest {
    BuildRequest {
        source: source.to_path_buf(),
        output_dir: output.to_path_buf(),
        language: None,
        name: Some("Hello".into()),
        mode,
        icon: None,
    }
}

fn run(path: &Path) -> std::process::Output {
    Command::new(path).output().expect("run artifact")
}

#[test]
fn cli_lists_python() {
    let bin = env!("CARGO_BIN_EXE_packager");
    let output = Command::new(bin).arg("languages").output().unwrap();
    assert!(output.status.success());
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("\"python\""));
    assert!(text.contains("\"zig\""));
}

#[test]
fn shell_app_prints_hello_and_leaves_the_source_tree_alone() {
    let root = workspace("shell");
    let source = root.join("project");
    fs::create_dir_all(&source).unwrap();
    let script = source.join("hello.sh");
    fs::write(&script, "#!/bin/bash\necho hello-packager\n").unwrap();
    let before = fs::read_dir(&source).unwrap().count();
    let output = root.join("out");
    fs::create_dir_all(&output).unwrap();
    let mut log = Capture { lines: Vec::new() };
    let outcome = build(&request(&script, &output, OutputMode::App), &mut log).unwrap();
    assert_eq!(outcome.language, Language::Shell);
    let app = &outcome.artifacts[0];
    let ran = run(&app.join("Contents/MacOS/hello"));
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "hello-packager\n");
    let after = fs::read_dir(&source).unwrap().count();
    assert_eq!(before, after);
}

#[test]
fn output_inside_the_project_is_rejected() {
    let root = workspace("overlap");
    let script = root.join("hello.sh");
    fs::write(&script, "#!/bin/bash\necho hi\n").unwrap();
    let output = root.join("out");
    fs::create_dir_all(&output).unwrap();
    let mut log = Capture { lines: Vec::new() };
    let err = build(&request(&script, &output, OutputMode::App), &mut log).unwrap_err();
    assert!(
        err.message().contains("inside the project"),
        "{}",
        err.message()
    );
}

#[test]
fn c_tool_prints_hello() {
    if !tool_installed("clang") {
        return;
    }
    let root = workspace("c");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    let source = project.join("main.c");
    fs::write(
        &source,
        "#include <stdio.h>\nint main(void){puts(\"hello-c\");}\n",
    )
    .unwrap();
    let output = root.join("out");
    fs::create_dir_all(&output).unwrap();
    let mut log = Capture { lines: Vec::new() };
    let outcome = build(&request(&source, &output, OutputMode::Tool), &mut log).expect("c build");
    assert_eq!(outcome.tool, "clang");
    let ran = run(&outcome.artifacts[0]);
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "hello-c\n");
}

#[test]
fn rust_file_tool_prints_hello() {
    if !tool_installed("rustc") {
        return;
    }
    let root = workspace("rust");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    let source = project.join("main.rs");
    fs::write(&source, "fn main(){println!(\"hello-rust\");}\n").unwrap();
    let output = root.join("out");
    fs::create_dir_all(&output).unwrap();
    let mut log = Capture { lines: Vec::new() };
    let outcome = build(&request(&source, &output, OutputMode::Tool), &mut log)
        .unwrap_or_else(|err: PackagerError| panic!("{err}\n{}", log.lines.join("\n")));
    let ran = run(&outcome.artifacts[0]);
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "hello-rust\n");
}

#[test]
fn node_tool_prints_hello() {
    if !tool_installed("node") {
        return;
    }
    let root = workspace("node");
    let project = root.join("project");
    fs::create_dir_all(&project).unwrap();
    let source = project.join("main.js");
    fs::write(&source, "console.log('hello-node')\n").unwrap();
    let output = root.join("out");
    fs::create_dir_all(&output).unwrap();
    let mut log = Capture { lines: Vec::new() };
    let outcome = build(&request(&source, &output, OutputMode::Tool), &mut log)
        .unwrap_or_else(|err| panic!("{err}\n{}", log.lines.join("\n")));
    let ran = run(&outcome.artifacts[0]);
    assert!(
        ran.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&ran.stderr),
        log.lines.join("\n")
    );
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "hello-node\n");
}
