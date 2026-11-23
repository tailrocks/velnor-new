// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use crate::error::CiError;

use super::crates_mirror::failed;

pub(super) const ENV: &str = "VELNOR_CARGO_MBX";
pub(super) const VALUE: &str = "required";
const MANIFEST_COMMANDS: &[&str] = &[
    "add",
    "b",
    "bench",
    "build",
    "c",
    "check",
    "clean",
    "clippy",
    "d",
    "doc",
    "fetch",
    "fix",
    "generate-lockfile",
    "locate-project",
    "metadata",
    "package",
    "pkgid",
    "publish",
    "r",
    "read-manifest",
    "remove",
    "rm",
    "run",
    "rustc",
    "rustdoc",
    "t",
    "test",
    "tree",
    "update",
    "vendor",
    "verify-project",
];

pub(super) fn required(args: &[OsString]) -> bool {
    recorded() && !nested_in_cargo() && declared_for_project(args)
}

fn nested_in_cargo() -> bool {
    std::env::var_os("CARGO_MANIFEST_DIR").is_some()
}

fn recorded() -> bool {
    std::env::var_os(ENV).is_some_and(|value| !value.is_empty())
        || super::crates_mirror_mode::recorded().is_ok_and(|mode| mode.is_some_and(|mode| mode.mbx))
}

fn declared_for_project(args: &[OsString]) -> bool {
    let Ok(work) = std::env::current_dir() else {
        return true;
    };
    let Ok(workspace) = workspace() else {
        return true;
    };
    let Some(directories) = project_directories(&work, args) else {
        return true;
    };
    directories.into_iter().any(|directory| {
        let directory = match std::fs::canonicalize(&directory) {
            Ok(resolved) => resolved,
            Err(_)
                if directory
                    .components()
                    .any(|part| part == Component::ParentDir) =>
            {
                return true;
            }
            Err(_) => directory,
        };
        scope(&directory, workspace.as_deref())
            .into_iter()
            .any(crate::mbx_declared::declared_or_unreadable_in)
    })
}

fn project_directories(launch: &Path, args: &[OsString]) -> Option<Vec<PathBuf>> {
    let work = invocation_directory(launch, args)?;
    let args = super::crates_mirror_alias::expand(&work, args)?;
    let command = subcommand(&args).map(|(_, command)| command);
    if let Some(target) = command.filter(|command| direct_target(command)) {
        let script = work.join(target);
        let script = std::fs::canonicalize(&script).unwrap_or(script);
        let mut directories = vec![
            script
                .parent()
                .map_or_else(|| work.clone(), Path::to_path_buf),
        ];
        if !leading(&args)?.script {
            directories.push(manifest_directory(&work, &args, false));
        }
        return Some(directories);
    }
    if command == Some("install")
        && let Some(path) = option_value(&args, "--path", false)
    {
        return Some(vec![work.join(path)]);
    }
    let shorthand = command.is_some_and(|command| MANIFEST_COMMANDS.contains(&command));
    Some(vec![manifest_directory(&work, &args, shorthand)])
}

fn manifest_directory(work: &Path, args: &[OsString], shorthand: bool) -> PathBuf {
    option_value(args, "--manifest-path", shorthand)
        .and_then(|manifest| work.join(manifest).parent().map(Path::to_path_buf))
        .unwrap_or_else(|| work.to_path_buf())
}

pub(super) fn direct_target(command: &str) -> bool {
    let path = Path::new(command);
    path.components().count() > 1 || path.extension().is_some_and(|extension| extension == "rs")
}

struct Leading<'a> {
    directory: Option<PathBuf>,
    script: bool,
    subcommand: Option<(usize, &'a str)>,
}

fn invocation_directory(launch: &Path, args: &[OsString]) -> Option<PathBuf> {
    let directory = leading(args)?.directory;
    let directory =
        directory.map_or_else(|| launch.to_path_buf(), |directory| launch.join(directory));
    std::fs::canonicalize(directory).ok()
}

pub(super) fn subcommand(args: &[OsString]) -> Option<(usize, &str)> {
    leading(args)?.subcommand
}

fn leading(args: &[OsString]) -> Option<Leading<'_>> {
    let mut directory = None;
    let mut script = false;
    let mut seen = args.iter().enumerate().take_while(|(_, arg)| *arg != "--");
    while let Some((index, arg)) = seen.next() {
        let arg = arg.to_str()?;
        if arg.starts_with('+') {
            continue;
        }
        if let Some(long) = arg.strip_prefix("--") {
            let (name, attached) = long
                .split_once('=')
                .map_or((long, false), |(name, _)| (name, true));
            match name {
                "color" | "config" | "explain" => {
                    if !attached {
                        seen.next()?;
                    }
                }
                "version" | "list" | "verbose" | "quiet" | "locked" | "offline" | "frozen"
                | "help"
                    if !attached => {}
                _ => return None,
            }
        } else if let Some(cluster) = arg.strip_prefix('-') {
            if cluster.is_empty() {
                return None;
            }
            for (offset, flag) in cluster.char_indices() {
                match flag {
                    'v' | 'q' | 'V' | 'h' => {}
                    'C' | 'Z' => {
                        let rest = &cluster[offset + 1..];
                        let value = if rest.is_empty() {
                            seen.next()?.1.to_str()?
                        } else {
                            rest.strip_prefix('=').unwrap_or(rest)
                        };
                        if value.is_empty() {
                            return None;
                        }
                        if flag == 'Z' && value == "script" {
                            script = true;
                        }
                        if flag == 'C' && directory.replace(PathBuf::from(value)).is_some() {
                            return None;
                        }
                        break;
                    }
                    _ => return None,
                }
            }
        } else {
            return Some(Leading {
                directory,
                script,
                subcommand: Some((index, arg)),
            });
        }
    }
    Some(Leading {
        directory,
        script,
        subcommand: None,
    })
}

pub(super) fn option_value(args: &[OsString], long: &str, shorthand: bool) -> Option<OsString> {
    let mut args = args.iter().take_while(|arg| *arg != "--");
    while let Some(arg) = args.next() {
        if arg == long || (shorthand && arg == "-m") {
            return args.next().cloned();
        }
        let Some(arg) = arg.to_str() else {
            continue;
        };
        if let Some(value) = arg
            .strip_prefix(long)
            .and_then(|rest| rest.strip_prefix('='))
        {
            return Some(value.into());
        }
        if shorthand
            && !arg.starts_with("--")
            && let Some(value) = arg.strip_prefix("-m")
        {
            return Some(value.strip_prefix('=').unwrap_or(value).into());
        }
    }
    None
}

fn workspace() -> std::io::Result<Option<PathBuf>> {
    std::env::var_os("GITHUB_WORKSPACE")
        .filter(|value| !value.is_empty())
        .map(|path| {
            let path = std::fs::canonicalize(path)?;
            if path.is_dir() {
                Ok(path)
            } else {
                Err(std::io::ErrorKind::NotADirectory.into())
            }
        })
        .transpose()
}

fn scope<'a>(directory: &'a Path, workspace: Option<&Path>) -> Vec<&'a Path> {
    match workspace {
        Some(workspace) if directory.starts_with(workspace) => directory
            .ancestors()
            .take_while(|ancestor| ancestor.starts_with(workspace))
            .collect(),
        _ => vec![directory],
    }
}

pub(super) fn refused(cargo: &Path) -> CiError {
    CiError::new(format!(
        "mbx is declared but cargo on PATH isn't the mise mbx wrapper: {}",
        cargo.display()
    ))
}

pub(super) fn wrapper_missing() -> CiError {
    CiError::new("mbx is declared but mise's cargo wrapper is missing after activation")
}

pub(super) fn declared_here() -> Result<bool, CiError> {
    let directory = std::env::current_dir().map_err(|error| failed(&error))?;
    crate::mbx_declared::checked_in(&directory).map_err(CiError::new)
}

pub(super) fn export_line(declared: bool) -> String {
    format!("{ENV}={}\n", if declared { VALUE } else { "" })
}
