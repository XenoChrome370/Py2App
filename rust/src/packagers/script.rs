use std::path::Path;

use crate::error::Result;
use crate::fsutil;
use crate::model::{BuildOutcome, BuildRequest, Detection, Language, Toolchain};
use crate::process::{self, Log};
use crate::product;

struct ScriptSpec {
    commands: &'static [&'static str],
    interpreter: &'static str,
    hint: &'static str,
    check: &'static [&'static str],
}

pub fn toolchain(language: Language) -> Toolchain {
    let spec = spec(language);
    match fsutil::find_tool(spec.commands) {
        Some(tool) => Toolchain {
            language: language.id().into(),
            label: language.label().into(),
            available: true,
            tool: spec.interpreter.into(),
            detail: format!("{} via {}", language.blurb(), tool.display()),
        },
        None => Toolchain {
            language: language.id().into(),
            label: language.label().into(),
            available: false,
            tool: spec.interpreter.into(),
            detail: spec.hint.into(),
        },
    }
}

pub fn build(
    request: &BuildRequest,
    detection: &Detection,
    display: &str,
    log: &mut dyn Log,
) -> Result<BuildOutcome> {
    let spec = spec(detection.language);
    let tool = fsutil::require_tool(spec.commands, spec.hint)?;
    let source = super::source_file(&detection.entry)?;
    if !spec.check.is_empty() {
        let mut args: Vec<String> = spec.check.iter().map(|arg| (*arg).to_string()).collect();
        args.push(source.display().to_string());
        process::run_ok(
            &tool,
            &args,
            Some(source.parent().unwrap_or(Path::new("."))),
            &[],
            log,
        )?;
    }
    let tool_name = fsutil::file_name(&tool);
    let interpreter = match detection.language {
        Language::Shell if fsutil::has_shebang(&source) => None,
        Language::Shell => Some("bash".to_string()),
        _ => Some(tool_name.clone()),
    };
    let artifacts =
        product::deliver_script(&source, interpreter.as_deref(), request, display, log)?;
    let runtime = interpreter.as_deref().unwrap_or("the shell");
    let note = match detection.language {
        Language::Shell => "The app runs this shell script.".to_string(),
        _ => format!("The app runs with {runtime} on the machine that opens it."),
    };
    Ok(super::outcome(
        detection.language,
        artifacts,
        spec.interpreter,
        vec![note],
    ))
}

fn spec(language: Language) -> ScriptSpec {
    match language {
        Language::Ruby => ScriptSpec {
            commands: &["ruby"],
            interpreter: "ruby",
            hint: "Install Ruby to package Ruby scripts",
            check: &["-c"],
        },
        Language::Php => ScriptSpec {
            commands: &["php"],
            interpreter: "php",
            hint: "Install PHP to package PHP scripts",
            check: &["-l"],
        },
        Language::Perl => ScriptSpec {
            commands: &["perl"],
            interpreter: "perl",
            hint: "Install Perl to package Perl scripts",
            check: &["-c"],
        },
        Language::Lua => ScriptSpec {
            commands: &["lua", "luajit"],
            interpreter: "lua",
            hint: "Install Lua to package Lua scripts",
            check: &[],
        },
        Language::Shell => ScriptSpec {
            commands: &["bash"],
            interpreter: "bash",
            hint: "bash is required to package shell scripts",
            check: &["-n"],
        },
        other => ScriptSpec {
            commands: &[],
            interpreter: other.id(),
            hint: "this language is not packaged as a script",
            check: &[],
        },
    }
}
