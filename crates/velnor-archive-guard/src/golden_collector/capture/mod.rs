// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod case;
mod dogfood;
mod fixture_git;
mod golden_manifest;
mod manifest;
mod output;

use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::Builder;

use super::filesystem;
use super::guard::Guard;
use super::{Arguments, CollectorError, binary_format};

const FIXTURES: [&str; 4] = ["nested", "mbx-nextest", "empty-suite", "minimal-cargo"];

#[derive(Clone, Copy, Debug)]
pub(super) enum Mode {
    Capture,
    Check,
}

struct Context<'a> {
    root: &'a Path,
    cli: &'a Path,
    guard: &'a Guard,
    workspace: &'a Path,
    version: &'a str,
    target: &'a str,
    cli_sha256: &'a str,
}

struct WorkspaceRun<'a> {
    mode: Mode,
    root: &'a Path,
    cli: &'a Path,
    golden_dir: &'a Path,
    guard: &'a Guard,
    workspace: &'a Path,
    target: &'a str,
    cli_sha256: &'a str,
}

pub(super) fn run(args: Arguments) -> Result<(), CollectorError> {
    let root = fs::canonicalize(args.root).map_err(CollectorError::operational)?;
    let cli = validate_cli(args.cli)?;
    let golden_dir = golden_dir(&root)?;
    let guard = Guard::admit(
        args.guard,
        args.guard_sha256,
        &root,
        args.command_timeout,
        args.output_cap_bytes,
    )?;
    let cli_sha256 = filesystem::sha256_file(&cli)?;
    let target = {
        let mut binary = fs::File::open(&cli).map_err(CollectorError::operational)?;
        let size = binary
            .metadata()
            .map_err(CollectorError::operational)?
            .len();
        binary_format::target(&mut binary, size).map_err(CollectorError::operational)?
    };
    let keep = keep_work()?;
    let temp = Builder::new()
        .prefix("velnor-goldens-work.")
        .tempdir()
        .map_err(CollectorError::operational)?;
    let workspace = fs::canonicalize(temp.path()).map_err(CollectorError::operational)?;
    let result = run_in_workspace(&WorkspaceRun {
        mode: args.mode,
        root: &root,
        cli: &cli,
        golden_dir: &golden_dir,
        guard: &guard,
        workspace: &workspace,
        target,
        cli_sha256: &cli_sha256,
    });
    if let Err(error) = result {
        retain(temp);
        return Err(error);
    }
    if keep {
        retain(temp);
    } else {
        temp.close().map_err(CollectorError::operational)?;
    }
    Ok(())
}

fn run_in_workspace(run: &WorkspaceRun<'_>) -> Result<(), CollectorError> {
    let WorkspaceRun {
        mode,
        root,
        cli,
        golden_dir,
        guard,
        workspace,
        target,
        cli_sha256,
    } = *run;
    let version = candidate_version(guard, root, cli, workspace)?;
    let context = Context {
        root,
        cli,
        guard,
        workspace,
        version: &version,
        target,
        cli_sha256,
    };
    let stage = workspace.join("stage");
    fs::create_dir(&stage).map_err(CollectorError::operational)?;
    for fixture in FIXTURES {
        let repo = workspace.join(fixture);
        let head = case::setup(&context, fixture, &repo)?;
        case::capture(&context, fixture, &repo, &head, &stage.join(fixture))?;
        println!("captured {fixture}");
    }
    let dogfood = dogfood::capture(&context, &stage.join("dogfood"))?;
    println!(
        "captured dogfood (tree {})",
        if dogfood { "identical" } else { "DIFFERS" }
    );
    match mode {
        Mode::Capture => capture_goldens(golden_dir, &stage, dogfood)?,
        Mode::Check => check_goldens(golden_dir, &stage)?,
    }
    Ok(())
}

fn capture_goldens(
    golden_dir: &Path,
    stage: &Path,
    dogfood_identical: bool,
) -> Result<(), CollectorError> {
    if !dogfood_identical {
        return Err(CollectorError::mismatch(
            "dogfood generated workflows are not identical to repository workflows",
        ));
    }
    ensure_real_directory(golden_dir)?;
    let cases = golden_dir.join("cases");
    remove_existing(&cases)?;
    fs::create_dir(&cases).map_err(CollectorError::operational)?;
    filesystem::copy_contents(stage, &cases)?;
    let tree = filesystem::read_tree(&cases)?;
    fs::write(
        golden_dir.join("MANIFEST.sha256"),
        filesystem::tree_hash(&tree),
    )
    .map_err(CollectorError::operational)?;
    println!("wrote {}", cases.display());
    Ok(())
}

fn check_goldens(golden_dir: &Path, stage: &Path) -> Result<(), CollectorError> {
    let mut mismatches = Vec::new();
    let cases_root = golden_dir.join("cases");
    match fs::symlink_metadata(&cases_root) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => {
            return Err(CollectorError::operational(
                "golden cases must be a real directory",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(CollectorError::mismatch(
                "golden cases directory is missing",
            ));
        }
        Err(error) => return Err(CollectorError::operational(error)),
    }
    golden_manifest::verify(golden_dir, &cases_root)?;
    for name in FIXTURES.into_iter().chain(["dogfood"]) {
        let golden = cases_root.join(name);
        let actual = stage.join(name);
        match fs::symlink_metadata(&golden) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => {
                mismatches.push(format!("{name}: golden case is not a real directory"));
                continue;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                mismatches.push(format!("{name}: missing golden"));
                continue;
            }
            Err(error) => return Err(CollectorError::operational(error)),
        }
        let expected = filesystem::read_tree(&golden)?;
        let observed = filesystem::read_tree(&actual)?;
        let differences = filesystem::compare(&expected, &observed);
        let link_diff =
            filesystem::link_identity(&expected) != filesystem::link_identity(&observed);
        if differences.is_empty() && !link_diff {
            println!("match: {name}");
        } else {
            let evidence = differences.join("\n");
            fs::write(stage.join(format!("{name}.diff")), &evidence)
                .map_err(CollectorError::operational)?;
            if link_diff {
                fs::write(
                    stage.join(format!("{name}.links.diff")),
                    format!(
                        "golden:\n{}stage:\n{}",
                        String::from_utf8_lossy(&filesystem::link_identity(&expected)),
                        String::from_utf8_lossy(&filesystem::link_identity(&observed))
                    ),
                )
                .map_err(CollectorError::operational)?;
            }
            eprintln!(
                "FAIL: golden mismatch: {name} ({} differing paths)",
                differences.len()
            );
            eprintln!(
                "{}",
                differences
                    .iter()
                    .take(20)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n")
            );
            mismatches.push(format!("{name}: {} differing paths", differences.len()));
        }
    }
    if mismatches.is_empty() {
        println!("ALL GOLDENS MATCH");
        Ok(())
    } else {
        Err(CollectorError::mismatch(mismatches.join("; ")))
    }
}

fn candidate_version(
    guard: &Guard,
    root: &Path,
    cli: &Path,
    workspace: &Path,
) -> Result<String, CollectorError> {
    let raw = workspace.join("raw/version");
    fs::create_dir_all(&raw).map_err(CollectorError::operational)?;
    let output = guard.run_candidate(root, Command::new(cli).arg("--version"), |result| {
        output::persist_result(&raw, "version", &raw, result)
    })?;
    if output.status != velnor_actions_mise::BoundedCommandStatus::Exited(0) {
        return Err(CollectorError::operational(format!(
            "candidate --version command failed ({:?})",
            output.status
        )));
    }
    let text = std::str::from_utf8(&output.stdout).map_err(CollectorError::operational)?;
    let mut lines = text.lines();
    let line = lines.next().unwrap_or_default();
    let mut fields = line.split_ascii_whitespace();
    let name = fields.next().unwrap_or_default();
    let version = fields.next().unwrap_or_default();
    if name != "velnor-actions"
        || fields.next().is_some()
        || lines.next().is_some()
        || !valid_version(version)
    {
        return Err(CollectorError::operational(
            "candidate --version output is malformed",
        ));
    }
    Ok(version.to_owned())
}

fn validate_cli(path: PathBuf) -> Result<PathBuf, CollectorError> {
    let metadata = fs::symlink_metadata(&path).map_err(CollectorError::operational)?;
    if !metadata.file_type().is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(CollectorError::usage(
            "--cli must identify a regular executable file",
        ));
    }
    fs::canonicalize(path).map_err(CollectorError::operational)
}

fn valid_version(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn keep_work() -> Result<bool, CollectorError> {
    match env::var("VELNOR_GOLDEN_KEEP_WORK").as_deref() {
        Ok("0") | Err(env::VarError::NotPresent) => Ok(false),
        Ok("1") => Ok(true),
        _ => Err(CollectorError::usage(
            "VELNOR_GOLDEN_KEEP_WORK must be 0 or 1",
        )),
    }
}

fn golden_dir(root: &Path) -> Result<PathBuf, CollectorError> {
    let caller = env::current_dir().map_err(CollectorError::operational)?;
    let external = env::var_os("VELNOR_GOLDEN_DIR").is_some();
    let requested = env::var_os("VELNOR_GOLDEN_DIR").map_or_else(
        || root.join("docs/proposed/opentofu-goldens"),
        PathBuf::from,
    );
    let requested = if requested.is_absolute() {
        requested
    } else {
        caller.join(requested)
    };
    let name = requested.file_name().ok_or_else(|| {
        CollectorError::usage("golden directory must have a final path component")
    })?;
    if name == "." || name == ".." {
        return Err(CollectorError::usage("golden directory name is invalid"));
    }
    let parent = requested
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::canonicalize(parent)
        .map_err(|_| CollectorError::operational("golden directory parent must exist"))?;
    let output = parent.join(name);
    if external && output.starts_with(root) {
        return Err(CollectorError::usage(
            "VELNOR_GOLDEN_DIR must be outside the repository",
        ));
    }
    match fs::symlink_metadata(&output) {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_) => {
            return Err(CollectorError::operational(
                "golden directory must be a real directory",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(CollectorError::operational(error)),
    }
    Ok(output)
}

fn ensure_real_directory(path: &Path) -> Result<(), CollectorError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(()),
        Ok(_) => Err(CollectorError::operational(
            "golden destination is not a real directory",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(path).map_err(CollectorError::operational)
        }
        Err(error) => Err(CollectorError::operational(error)),
    }
}

fn remove_existing(path: &Path) -> Result<(), CollectorError> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if metadata.file_type().is_dir() {
        fs::remove_dir_all(path).map_err(CollectorError::operational)
    } else {
        fs::remove_file(path).map_err(CollectorError::operational)
    }
}

fn retain(temp: tempfile::TempDir) {
    let path = temp.keep();
    eprintln!("retaining golden evidence workspace: {}", path.display());
}
