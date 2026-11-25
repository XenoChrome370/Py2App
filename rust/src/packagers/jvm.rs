use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{PackagerError, Result};
use crate::fsutil::{self, TempDir};
use crate::model::{BuildOutcome, BuildRequest, Detection, Language, OutputMode, Toolchain};
use crate::process::{self, Log};
use crate::product;

pub fn toolchain(language: Language) -> Toolchain {
    let available = match language {
        Language::Kotlin => {
            fsutil::find_tool(&["kotlinc"]).is_some() || fsutil::find_tool(&["gradle"]).is_some()
        }
        _ => {
            fsutil::find_tool(&["javac"]).is_some()
                || fsutil::find_tool(&["mvn"]).is_some()
                || fsutil::find_tool(&["gradle"]).is_some()
        }
    };
    let tool = if language == Language::Kotlin && fsutil::find_tool(&["kotlinc"]).is_some() {
        "kotlinc"
    } else if fsutil::find_tool(&["jpackage"]).is_some() {
        "jpackage"
    } else if fsutil::find_tool(&["javac"]).is_some() {
        "javac"
    } else {
        "java"
    };
    Toolchain {
        language: language.id().into(),
        label: language.label().into(),
        available,
        tool: tool.into(),
        detail: if available {
            "Maven, Gradle, javac, or kotlinc, then jpackage when it is installed".into()
        } else {
            format!("Install a JDK to package {}", language.label())
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
    if root.join("pom.xml").is_file() {
        return maven(request, root, display, detection.language, log);
    }
    if root.join("build.gradle").is_file() || root.join("build.gradle.kts").is_file() {
        return gradle(request, root, display, detection.language, log);
    }
    if detection.language == Language::Kotlin
        || fsutil::extension_of(&detection.entry).as_deref() == Some("kt")
    {
        return kotlinc_file(request, detection, display, log);
    }
    javac_file(request, detection, display, log)
}

fn maven(
    request: &BuildRequest,
    root: &Path,
    display: &str,
    language: Language,
    log: &mut dyn Log,
) -> Result<BuildOutcome> {
    let mvnw = root.join("mvnw");
    if mvnw.is_file() {
        let bash = fsutil::require_tool(&["bash"], "bash is required to run mvnw")?;
        process::run_ok(
            &bash,
            &[
                mvnw.display().to_string(),
                "-q".into(),
                "-DskipTests".into(),
                "package".into(),
            ],
            Some(root),
            &[],
            log,
        )?;
    } else {
        let mvn = fsutil::require_tool(&["mvn"], "Install Maven or add mvnw to the project")?;
        process::run_ok(
            &mvn,
            &["-q".into(), "-DskipTests".into(), "package".into()],
            Some(root),
            &[],
            log,
        )?;
    }
    let jar = largest_jar(&root.join("target"))?;
    emit_jar(
        request,
        display,
        &jar,
        jar_main_class(&jar),
        "maven",
        language,
        log,
    )
}

fn gradle(
    request: &BuildRequest,
    root: &Path,
    display: &str,
    language: Language,
    log: &mut dyn Log,
) -> Result<BuildOutcome> {
    let gradlew = root.join("gradlew");
    if gradlew.is_file() {
        let bash = fsutil::require_tool(&["bash"], "bash is required to run gradlew")?;
        process::run_ok(
            &bash,
            &[
                gradlew.display().to_string(),
                "build".into(),
                "-x".into(),
                "test".into(),
                "--console=plain".into(),
            ],
            Some(root),
            &[],
            log,
        )?;
    } else {
        let gradle =
            fsutil::require_tool(&["gradle"], "Install Gradle or add gradlew to the project")?;
        process::run_ok(
            &gradle,
            &[
                "build".into(),
                "-x".into(),
                "test".into(),
                "--console=plain".into(),
            ],
            Some(root),
            &[],
            log,
        )?;
    }
    let jar = largest_jar(&root.join("build/libs"))?;
    emit_jar(
        request,
        display,
        &jar,
        jar_main_class(&jar),
        "gradle",
        language,
        log,
    )
}

fn javac_file(
    request: &BuildRequest,
    detection: &Detection,
    display: &str,
    log: &mut dyn Log,
) -> Result<BuildOutcome> {
    let javac = fsutil::require_tool(&["javac"], "Install a JDK to package Java")?;
    let jar_tool = fsutil::require_tool(&["jar"], "Install a JDK to package Java")?;
    let source = super::source_file(&detection.entry)?;
    let class_name = java_class_name(&source)?;
    let temp = TempDir::new("packager-javac")?;
    let classes = temp.path().join("classes");
    fs::create_dir_all(&classes)?;
    process::run_ok(
        &javac,
        &[
            "-d".into(),
            classes.display().to_string(),
            source.display().to_string(),
        ],
        Some(source.parent().unwrap_or(Path::new("."))),
        &[],
        log,
    )?;
    let jar_path = temp.path().join("app.jar");
    process::run_ok(
        &jar_tool,
        &[
            "cfe".into(),
            jar_path.display().to_string(),
            class_name.clone(),
            "-C".into(),
            classes.display().to_string(),
            ".".into(),
        ],
        Some(temp.path()),
        &[],
        log,
    )?;
    emit_jar(
        request,
        display,
        &jar_path,
        Some(class_name),
        "javac",
        Language::Java,
        log,
    )
}

fn kotlinc_file(
    request: &BuildRequest,
    detection: &Detection,
    display: &str,
    log: &mut dyn Log,
) -> Result<BuildOutcome> {
    let kotlinc = fsutil::require_tool(&["kotlinc"], "Install kotlinc to package Kotlin files")?;
    let source = super::source_file(&detection.entry)?;
    let class_name = kotlin_class_name(&source);
    let temp = TempDir::new("packager-kotlinc")?;
    let jar_path = temp.path().join("app.jar");
    process::run_ok(
        &kotlinc,
        &[
            source.display().to_string(),
            "-include-runtime".into(),
            "-d".into(),
            jar_path.display().to_string(),
        ],
        Some(source.parent().unwrap_or(Path::new("."))),
        &[],
        log,
    )?;
    emit_jar(
        request,
        display,
        &jar_path,
        Some(class_name),
        "kotlinc",
        Language::Kotlin,
        log,
    )
}

fn emit_jar(
    request: &BuildRequest,
    display: &str,
    jar_path: &Path,
    main_class: Option<String>,
    tool: &str,
    language: Language,
    log: &mut dyn Log,
) -> Result<BuildOutcome> {
    if request.mode == OutputMode::App {
        if let Some(jpackage) = fsutil::find_tool(&["jpackage"]) {
            if let Some(app) = try_jpackage(
                request,
                display,
                jar_path,
                main_class.as_deref(),
                &jpackage,
                log,
            )? {
                return Ok(super::outcome(
                    language,
                    vec![app],
                    "jpackage",
                    vec!["The app includes a Java runtime.".into()],
                ));
            }
        }
    }
    let temp = TempDir::new("packager-java-app")?;
    let payload = temp.path().join("payload");
    fs::create_dir_all(&payload)?;
    fsutil::copy_file(jar_path, &payload.join("app.jar"))?;
    let launcher = payload.join("launch");
    fsutil::write_file(&launcher, &java_launcher(main_class.as_deref()))?;
    fsutil::make_executable(&launcher)?;
    let artifacts = product::deliver_tree(&payload, &launcher, request, display, log)?;
    Ok(super::outcome(
        language,
        artifacts,
        tool,
        vec!["The app runs with Java on the machine that opens it.".into()],
    ))
}

fn try_jpackage(
    request: &BuildRequest,
    display: &str,
    jar_path: &Path,
    main_class: Option<&str>,
    jpackage: &Path,
    log: &mut dyn Log,
) -> Result<Option<PathBuf>> {
    let Some(main_class) = main_class else {
        log.line("jpackage needs a Main-Class. Writing a Java launcher instead.");
        return Ok(None);
    };
    let temp = TempDir::new("packager-jpackage")?;
    let input = temp.path().join("input");
    fs::create_dir_all(&input)?;
    fsutil::copy_file(jar_path, &input.join("app.jar"))?;
    let dest_app = request.output_dir.join(format!("{display}.app"));
    if dest_app.exists() {
        fs::remove_dir_all(&dest_app)?;
    }
    let mut args = vec![
        "--type".into(),
        "app-image".into(),
        "--name".into(),
        display.to_string(),
        "--input".into(),
        input.display().to_string(),
        "--main-jar".into(),
        "app.jar".into(),
        "--main-class".into(),
        main_class.to_string(),
        "--dest".into(),
        request.output_dir.display().to_string(),
    ];
    if let Some(icon) = &request.icon {
        if fsutil::extension_of(icon).as_deref() == Some("icns") {
            args.push("--icon".into());
            args.push(icon.display().to_string());
        }
    }
    match process::run(jpackage, &args, Some(&request.output_dir), &[], log)? {
        0 if dest_app.is_dir() => Ok(Some(dest_app)),
        0 => Ok(find_app(&request.output_dir, display)),
        _ => {
            log.line("jpackage did not produce an app. Writing a Java launcher instead.");
            Ok(None)
        }
    }
}

fn find_app(dir: &Path, display: &str) -> Option<PathBuf> {
    let expected = dir.join(format!("{display}.app"));
    if expected.is_dir() {
        Some(expected)
    } else {
        None
    }
}

fn java_launcher(main_class: Option<&str>) -> String {
    let command = match main_class {
        Some(class) => format!("exec java -cp \"$HERE/app.jar\" {class} \"$@\""),
        None => "exec java -jar \"$HERE/app.jar\" \"$@\"".to_string(),
    };
    format!(
        "#!/bin/bash\nset -euo pipefail\nHERE=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\ncd \"$HERE\"\n{command}\n"
    )
}

fn largest_jar(dir: &Path) -> Result<PathBuf> {
    let mut jars = Vec::new();
    collect_jars(dir, 2, &mut jars);
    jars.retain(|path| {
        let name = fsutil::file_name(path).to_ascii_lowercase();
        !name.contains("sources") && !name.contains("javadoc") && !name.contains("-tests")
    });
    let plain: Vec<_> = jars
        .iter()
        .filter(|path| fsutil::file_name(path).contains("-plain"))
        .cloned()
        .collect();
    if jars.len() > plain.len() {
        jars.retain(|path| !fsutil::file_name(path).contains("-plain"));
    }
    jars.into_iter()
        .max_by_key(|path| fs::metadata(path).map(|meta| meta.len()).unwrap_or(0))
        .ok_or_else(|| PackagerError::msg(format!("no jar was produced in {}", dir.display())))
}

fn collect_jars(dir: &Path, depth: usize, jars: &mut Vec<PathBuf>) {
    if depth == 0 || !dir.is_dir() {
        return;
    }
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jars(&path, depth - 1, jars);
        } else if fsutil::extension_of(&path).as_deref() == Some("jar") {
            jars.push(path);
        }
    }
}

fn jar_main_class(jar: &Path) -> Option<String> {
    let unzip = fsutil::find_tool(&["unzip"])?;
    let output = std::process::Command::new(unzip)
        .args(["-p"])
        .arg(jar)
        .arg("META-INF/MANIFEST.MF")
        .env("PATH", fsutil::enriched_path())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_main_class(&String::from_utf8_lossy(&output.stdout))
}

fn parse_main_class(manifest: &str) -> Option<String> {
    let mut key = String::new();
    let mut value = String::new();
    for line in manifest.lines() {
        if let Some(rest) = line.strip_prefix(' ') {
            value.push_str(rest.trim());
            continue;
        }
        if key.eq_ignore_ascii_case("Main-Class") && !value.is_empty() {
            return Some(value);
        }
        if let Some((next_key, next_value)) = line.split_once(':') {
            key = next_key.trim().to_string();
            value = next_value.trim().to_string();
        }
    }
    if key.eq_ignore_ascii_case("Main-Class") && !value.is_empty() {
        Some(value)
    } else {
        None
    }
}

fn java_class_name(source: &Path) -> Result<String> {
    let text = fsutil::read_to_string(source)?;
    let package = text.lines().find_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix("package ")?;
        Some(rest.trim().trim_end_matches(';').trim().to_string())
    });
    let stem = source
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    if stem.is_empty() {
        return Err(PackagerError::msg("the Java file has no class name"));
    }
    Ok(match package {
        Some(package) if !package.is_empty() => format!("{package}.{stem}"),
        _ => stem,
    })
}

fn kotlin_class_name(source: &Path) -> String {
    let text = fsutil::read_to_string(source).unwrap_or_default();
    let package = text.lines().find_map(|line| {
        let line = line.trim();
        let rest = line.strip_prefix("package ")?;
        Some(rest.trim().trim_end_matches(';').trim().to_string())
    });
    let stem = source.file_stem().unwrap_or_default().to_string_lossy();
    let class_name = format!("{stem}Kt");
    match package {
        Some(package) if !package.is_empty() => format!("{package}.{class_name}"),
        _ => class_name,
    }
}
