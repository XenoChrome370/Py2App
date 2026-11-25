use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{PackagerError, Result};
use crate::fsutil::{self, TempDir};
use crate::model::{executable_slug, BuildOutcome, BuildRequest, Detection, Language, Toolchain};
use crate::process::{self, Log};
use crate::product;

pub fn toolchain() -> Toolchain {
    match fsutil::find_tool(&["dotnet"]) {
        Some(dotnet) => Toolchain {
            language: "csharp".into(),
            label: "C#".into(),
            available: true,
            tool: "dotnet".into(),
            detail: dotnet.display().to_string(),
        },
        None => Toolchain {
            language: "csharp".into(),
            label: "C#".into(),
            available: false,
            tool: "dotnet".into(),
            detail: "Install the .NET SDK to package C# projects".into(),
        },
    }
}

pub fn build(
    request: &BuildRequest,
    detection: &Detection,
    display: &str,
    log: &mut dyn Log,
) -> Result<BuildOutcome> {
    let dotnet = fsutil::require_tool(&["dotnet"], "Install the .NET SDK to package C# projects")?;
    let temp = TempDir::new("packager-dotnet")?;
    let project = ensure_project(detection, display, temp.path())?;
    let publish = temp.path().join("publish");
    process::run_ok(
        &dotnet,
        &[
            "publish".into(),
            project.display().to_string(),
            "-c".into(),
            "Release".into(),
            "-o".into(),
            publish.display().to_string(),
            "--nologo".into(),
        ],
        Some(project.parent().unwrap_or(Path::new("."))),
        &[],
        log,
    )?;
    let executable = find_apphost(&publish, &executable_slug(display))?;
    let artifacts = product::deliver_tree(&publish, &executable, request, display, log)?;
    Ok(super::outcome(
        Language::CSharp,
        artifacts,
        "dotnet",
        vec!["The app runs with the .NET runtime installed on the machine that opens it.".into()],
    ))
}

fn ensure_project(detection: &Detection, display: &str, temp: &Path) -> Result<PathBuf> {
    let entry_name = fsutil::file_name(&detection.entry);
    if detection.entry.is_file()
        && (entry_name.ends_with(".csproj")
            || entry_name.ends_with(".fsproj")
            || entry_name.ends_with(".sln"))
    {
        return Ok(detection.entry.clone());
    }
    if detection.entry.is_dir() || detection.project_root.join(".").is_dir() {
        let projects = project_files(&detection.project_root);
        match projects.as_slice() {
            [only] => return Ok(only.clone()),
            [] => {}
            many => {
                return Err(PackagerError::msg(format!(
                    "several C# projects in {}. Pick one, for example {}",
                    detection.project_root.display(),
                    many[0].display()
                )));
            }
        }
    }
    let source = if detection.entry.is_file() {
        detection.entry.clone()
    } else {
        let files = fsutil::files_with_extensions(&detection.project_root, &["cs"], 2);
        if files.len() == 1 {
            files[0].clone()
        } else {
            return Err(PackagerError::msg("choose a .csproj or a single .cs file"));
        }
    };
    let folder = temp.join("project");
    fs::create_dir_all(&folder)?;
    fsutil::copy_file(&source, &folder.join(fsutil::file_name(&source)))?;
    let slug = executable_slug(display);
    let csproj = folder.join(format!("{slug}.csproj"));
    fsutil::write_file(&csproj, &csproj_text(&slug, &target_framework()))?;
    Ok(csproj)
}

fn project_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(read) = fs::read_dir(dir) {
        for entry in read.flatten() {
            let path = entry.path();
            let name = fsutil::file_name(&path);
            if name.ends_with(".csproj") || name.ends_with(".fsproj") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn target_framework() -> String {
    let Some(dotnet) = fsutil::find_tool(&["dotnet"]) else {
        return "net8.0".into();
    };
    let version = process::output_text(&dotnet, &["--version"]).unwrap_or_default();
    let major = version.split('.').next().unwrap_or("8");
    if major.chars().all(|ch| ch.is_ascii_digit()) && !major.is_empty() {
        format!("net{major}.0")
    } else {
        "net8.0".into()
    }
}

fn csproj_text(name: &str, framework: &str) -> String {
    format!(
        r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>{framework}</TargetFramework>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <AssemblyName>{name}</AssemblyName>
  </PropertyGroup>
</Project>
"#
    )
}

fn find_apphost(publish: &Path, slug: &str) -> Result<PathBuf> {
    let direct = publish.join(slug);
    if direct.is_file() {
        return Ok(direct);
    }
    let mut executables = fsutil::executables_under(publish, 1);
    executables.retain(|path| {
        let name = fsutil::file_name(path).to_ascii_lowercase();
        name != "createdump" && !name.ends_with(".dll") && !name.ends_with(".json")
    });
    executables
        .into_iter()
        .next()
        .ok_or_else(|| PackagerError::msg("dotnet publish finished without an executable"))
}
