use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{PackagerError, Result};
use crate::fsutil::{self, TempDir};
use crate::model::{BuildOutcome, BuildRequest, Detection, Language, Toolchain};
use crate::process::{self, Log};
use crate::product;

pub fn toolchain(language: Language) -> Toolchain {
    let tool_name = if language == Language::Cpp {
        "clang++"
    } else {
        "clang"
    };
    let found = fsutil::find_tool(&[tool_name]).or_else(|| fsutil::find_tool(&["cmake", "make"]));
    match found {
        Some(tool) => Toolchain {
            language: language.id().into(),
            label: language.label().into(),
            available: true,
            tool: tool_name.into(),
            detail: tool.display().to_string(),
        },
        None => Toolchain {
            language: language.id().into(),
            label: language.label().into(),
            available: false,
            tool: tool_name.into(),
            detail: format!("Install the Xcode command line tools for {tool_name}"),
        },
    }
}

pub fn build(
    request: &BuildRequest,
    detection: &Detection,
    display: &str,
    log: &mut dyn Log,
) -> Result<BuildOutcome> {
    let root = &detection.project_root;
    if root.join("CMakeLists.txt").is_file() {
        return cmake_build(request, root, display, log);
    }
    if root.join("Makefile").is_file()
        || root.join("makefile").is_file()
        || root.join("GNUmakefile").is_file()
    {
        return make_build(request, root, display, log);
    }
    compile_sources(request, detection, display, log)
}

fn cmake_build(
    request: &BuildRequest,
    root: &Path,
    display: &str,
    log: &mut dyn Log,
) -> Result<BuildOutcome> {
    let cmake = fsutil::require_tool(&["cmake"], "Install CMake to package this project")?;
    let temp = TempDir::new("packager-cmake")?;
    let build_dir = temp.path().join("build");
    process::run_ok(
        &cmake,
        &[
            "-S".into(),
            root.display().to_string(),
            "-B".into(),
            build_dir.display().to_string(),
            "-DCMAKE_BUILD_TYPE=Release".into(),
        ],
        Some(root),
        &[],
        log,
    )?;
    process::run_ok(
        &cmake,
        &[
            "--build".into(),
            build_dir.display().to_string(),
            "--config".into(),
            "Release".into(),
        ],
        Some(root),
        &[],
        log,
    )?;
    let binary = choose_binary(fsutil::executables_under(&build_dir, 4))?;
    let artifacts = product::deliver_binary(&binary, request, display, log)?;
    Ok(super::outcome(
        if detection_cpp(root) {
            Language::Cpp
        } else {
            Language::C
        },
        artifacts,
        "cmake",
        vec!["Native release build.".into()],
    ))
}

fn make_build(
    request: &BuildRequest,
    root: &Path,
    display: &str,
    log: &mut dyn Log,
) -> Result<BuildOutcome> {
    let make = fsutil::require_tool(&["make"], "Install make to package this project")?;
    let before = fsutil::executables_under(root, 3);
    process::run_ok(
        &make,
        &["-C".into(), root.display().to_string()],
        Some(root),
        &[],
        log,
    )?;
    let after = fsutil::executables_under(root, 3);
    let fresh: Vec<_> = after
        .into_iter()
        .filter(|path| !before.iter().any(|old| old == path))
        .collect();
    let pool = if fresh.is_empty() { before } else { fresh };
    let binary = choose_binary(pool)?;
    let artifacts = product::deliver_binary(&binary, request, display, log)?;
    Ok(super::outcome(
        if detection_cpp(root) {
            Language::Cpp
        } else {
            Language::C
        },
        artifacts,
        "make",
        vec!["Native build from the Makefile.".into()],
    ))
}

fn compile_sources(
    request: &BuildRequest,
    detection: &Detection,
    display: &str,
    log: &mut dyn Log,
) -> Result<BuildOutcome> {
    let cpp = detection.language == Language::Cpp;
    let compiler = if cpp {
        fsutil::require_tool(&["clang++"], "Install clang++ to package C++")?
    } else {
        fsutil::require_tool(&["clang"], "Install clang to package C")?
    };
    let sources = if detection.entry.is_file() {
        vec![detection.entry.clone()]
    } else {
        fsutil::files_with_extensions(&detection.project_root, detection.language.extensions(), 1)
    };
    if sources.is_empty() {
        return Err(PackagerError::msg("no C or C++ source files to compile"));
    }
    let temp = TempDir::new("packager-clang")?;
    let binary = temp.path().join("app");
    let mut args = vec!["-O2".into(), "-o".into(), binary.display().to_string()];
    if cpp {
        args.push("-std=c++17".into());
    }
    for source in &sources {
        args.push(source.display().to_string());
    }
    process::run_ok(&compiler, &args, Some(&detection.project_root), &[], log)?;
    let artifacts = product::deliver_binary(&binary, request, display, log)?;
    Ok(super::outcome(
        detection.language,
        artifacts,
        if cpp { "clang++" } else { "clang" },
        vec!["Native executable.".into()],
    ))
}

fn choose_binary(mut binaries: Vec<PathBuf>) -> Result<PathBuf> {
    binaries.retain(|path| {
        let name = fsutil::file_name(path).to_ascii_lowercase();
        !name.contains("cmake") && name != "a.out" && !name.ends_with(".dsym")
    });
    if binaries.is_empty() {
        return Err(PackagerError::msg(
            "the build finished without an executable",
        ));
    }
    let preferred: Vec<_> = binaries
        .iter()
        .filter(|path| {
            let name = fsutil::file_name(path).to_ascii_lowercase();
            !name.starts_with("test") && !name.contains("_test")
        })
        .cloned()
        .collect();
    let pool = if preferred.is_empty() {
        binaries
    } else {
        preferred
    };
    pool.into_iter()
        .max_by_key(|path| fs::metadata(path).map(|meta| meta.len()).unwrap_or(0))
        .ok_or_else(|| PackagerError::msg("the build finished without an executable"))
}

fn detection_cpp(root: &Path) -> bool {
    !fsutil::files_with_extensions(root, Language::Cpp.extensions(), 3).is_empty()
}
