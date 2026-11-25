// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use super::super::super::units::{MetadataInvocation, Workspace, refuse_arguments};
use super::super::groups::root_workspace;
use crate::error::CiError;

mod alias;
mod derived;
mod package_spec;
mod platform;
mod resolve;
mod root;
mod selection;
mod wanted;
pub(super) use alias::resolve;
use derived::{build_of, refusal, run_program_arguments, runs_doctests, without_open};
pub(super) use platform::Platform;
pub(super) use root::{invoked_root, root_directory};
pub(super) use selection::Selection;

const CARGO_PROGRAMS: [&str; 3] = ["cargo", "cargo-nextest", "cargo-clippy"];

const CARGO_SUBCOMMANDS: [&str; 16] = [
    "build", "b", "check", "c", "clippy", "test", "t", "nextest", "doc", "d", "run", "r", "bench",
    "rustc", "fetch", "metadata",
];

pub(super) struct Invocation {
    pub(super) command: Vec<String>,
    pub(super) preflight: Option<Vec<String>>,
    pub(super) compiler: bool,
    pub(super) cargo: bool,
    builds: bool,
    refusal: Option<&'static str>,
    globals: Vec<String>,
    chdir: Option<PathBuf>,
}

impl Invocation {
    pub(super) fn label(&self) -> String {
        let program = Path::new(&self.command[0])
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| CARGO_PROGRAMS.contains(name))
            .unwrap_or("command");
        if program != "cargo" {
            return program.to_owned();
        }
        let words: Vec<&str> = self
            .command
            .iter()
            .skip(1)
            .map(String::as_str)
            .take_while(|word| *word != "--")
            .collect();
        match subcommand(&words)
            .map(|index| words[index])
            .filter(|word| CARGO_SUBCOMMANDS.contains(word))
        {
            Some(word) => format!("cargo {word}"),
            None => program.to_owned(),
        }
    }

    fn cargo_program(&self) -> bool {
        Path::new(&self.command[0])
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| matches!(name, "cargo" | "cargo-nextest" | "cargo-clippy"))
    }

    pub(super) fn new(command: &[String]) -> Self {
        let program = Path::new(&command[0])
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let words: Vec<&str> = command
            .iter()
            .skip(1)
            .map(String::as_str)
            .take_while(|word| *word != "--")
            .collect();
        let start = if program == "cargo" {
            subcommand(&words)
        } else {
            Some(0)
        };
        let arguments = start
            .and_then(|start| words.get(start..))
            .unwrap_or_default();
        let nextest = matches!(
            (program, arguments),
            ("cargo" | "cargo-nextest", ["nextest", "run", ..])
        );
        let test = matches!(
            (program, arguments),
            ("cargo", ["test" | "t" | "bench", ..])
        );
        let doctest = test && words.contains(&"--doc");
        let compilation = matches!(
            (program, arguments),
            (
                "cargo",
                [
                    "build" | "b" | "check" | "c" | "clippy" | "doc" | "d" | "rustc" | "rustdoc",
                    ..
                ]
            ) | ("cargo-clippy", ["clippy", ..])
                | ("cargo" | "cargo-nextest", ["nextest", "archive", ..])
        );
        let doctests = program == "cargo" && runs_doctests(arguments, command);
        let compiler = compilation && !words.contains(&"--open") && !doctests
            || (nextest || test && !doctest) && words.contains(&"--no-run");
        let running = matches!((program, arguments), ("cargo", ["run" | "r", ..]));
        let preflight = if running {
            start.map(|start| build_of(command, start))
        } else if compilation && words.contains(&"--open") && !doctests {
            Some(without_open(command))
        } else {
            ((nextest || test && !doctest) && !compiler).then(|| {
                let end = command
                    .iter()
                    .position(|word| word == "--")
                    .unwrap_or(command.len());
                let mut preflight: Vec<String> = command[..end]
                    .iter()
                    .filter(|word| !matches!(word.as_str(), "--no-fail-fast" | "--fail-fast"))
                    .cloned()
                    .collect();
                preflight.push("--no-run".to_owned());
                preflight.extend(command[end..].iter().cloned());
                preflight
            })
        };
        let mut globals = if program == "cargo" {
            start.map_or_else(Vec::new, |start| command[1..=start].to_vec())
        } else {
            Vec::new()
        };
        let mut remaining = arguments.iter().skip(1);
        while let Some(word) = remaining.next() {
            if *word == "--config" {
                globals.push((*word).to_owned());
                if let Some(value) = remaining.next() {
                    globals.push((*value).to_owned());
                }
            } else if word.starts_with("--config=") {
                globals.push((*word).to_owned());
            }
        }
        let chdir = start.and_then(|start| chdir(&words[..start]));
        let refusal = refusal(
            program,
            arguments,
            running,
            compilation || nextest || test || running,
            &words[..start.filter(|_| running).map_or(words.len(), |start| {
                (run_program_arguments(command, start) - 1).min(words.len())
            })],
        );
        Self {
            command: command.to_vec(),
            preflight,
            compiler,
            cargo: compilation || nextest || test || running,
            builds: compilation || nextest || test || running,
            refusal,
            globals,
            chdir,
        }
    }

    fn work(&self, directory: &Path) -> PathBuf {
        self.chdir
            .as_ref()
            .map_or_else(|| directory.to_path_buf(), |chdir| directory.join(chdir))
    }

    pub(super) fn step(&self, directory: &Path) -> PathBuf {
        self.chdir
            .as_ref()
            .filter(|_| self.builds)
            .map_or_else(|| directory.to_path_buf(), |chdir| directory.join(chdir))
    }

    pub(super) fn location(&self, directory: &Path) -> PathBuf {
        if !self.builds {
            return directory.to_path_buf();
        }
        let base = self.step(directory);
        self.command
            .iter()
            .enumerate()
            .take_while(|(_, word)| word.as_str() != "--")
            .find_map(|(index, word)| {
                word.strip_prefix("--manifest-path=")
                    .map(PathBuf::from)
                    .or_else(|| {
                        (word == "--manifest-path")
                            .then(|| self.command.get(index + 1).map(PathBuf::from))
                            .flatten()
                    })
            })
            .map_or_else(
                || base.clone(),
                |manifest| base.join(manifest).parent().unwrap_or(&base).to_path_buf(),
            )
    }

    pub(super) fn validate_targets(
        &self,
        directory: &Path,
        workspaces: &[Workspace],
        standalone: &[bool],
        inherit: &[bool],
        groups: &[PathBuf],
    ) -> Result<(), CiError> {
        if let Some(refusal) = self.refusal {
            return Err(CiError::new(refusal));
        }
        if !self.builds {
            return Ok(());
        }
        if self
            .command
            .iter()
            .take_while(|word| word.as_str() != "--")
            .any(|word| word == "--target-dir" || word.starts_with("--target-dir="))
        {
            return Err(CiError::new(
                "inputs build does not accept a command-local --target-dir; use persistent Cargo configuration or CARGO_TARGET_DIR for both build and inputs audit",
            ));
        }
        refuse_arguments(
            self.command
                .iter()
                .map(String::as_str)
                .take_while(|word| *word != "--"),
        )?;
        let executable = if Path::new(&self.command[0])
            .file_name()
            .is_some_and(|name| name == "cargo")
        {
            self.command[0].as_str()
        } else {
            "cargo"
        };
        let invocation = MetadataInvocation {
            executable,
            arguments: &self.globals,
            directory,
        };
        let local_arguments = without_command_scope(&self.globals);
        for ((expected, alone), inherit) in workspaces.iter().zip(standalone).zip(inherit) {
            let manifest = expected.root.join("Cargo.toml");
            let local = MetadataInvocation {
                executable,
                arguments: &local_arguments,
                directory: &expected.root,
            };
            let actual = root_workspace(
                &manifest,
                Some(if *inherit { &invocation } else { &local }),
                directory,
                *inherit,
                *alone,
                groups,
            )?;
            if actual.root != expected.root
                || actual.target_directory != expected.target_directory
                || actual.build_directory != expected.build_directory
            {
                return Err(CiError::new(
                    "Cargo invocation configuration redirects the workspace, target or build directory away from inputs audit; use persistent Cargo configuration, CARGO_TARGET_DIR or CARGO_BUILD_BUILD_DIR for both build and audit",
                ));
            }
        }
        Ok(())
    }
}

fn without_command_scope(globals: &[String]) -> Vec<String> {
    let mut kept = Vec::new();
    let mut words = globals.iter();
    while let Some(word) = words.next() {
        if word == "-C" || word == "--config" {
            words.next();
        } else if !word.starts_with("-C") && !word.starts_with("--config=") {
            kept.push(word.clone());
        }
    }
    kept
}

fn chdir(globals: &[&str]) -> Option<PathBuf> {
    let mut directory = None;
    let mut index = 0;
    while let Some(word) = globals.get(index) {
        match *word {
            "-C" => {
                directory = globals.get(index + 1).map(PathBuf::from);
                index += 2;
            }
            "--config" | "-Z" | "--color" | "-j" | "--jobs" | "--manifest-path" => index += 2,
            value if value.starts_with("-C") => {
                let attached = &value[2..];
                directory = Some(PathBuf::from(
                    attached.strip_prefix('=').unwrap_or(attached),
                ));
                index += 1;
            }
            _ => index += 1,
        }
    }
    directory
}

fn subcommand(words: &[&str]) -> Option<usize> {
    let mut index = usize::from(
        words
            .first()
            .is_some_and(|word| word.starts_with('+') && word.len() > 1),
    );
    while let Some(word) = words.get(index) {
        match *word {
            "-q" | "--quiet" | "--verbose" | "--locked" | "--offline" | "--frozen" => index += 1,
            "--color" | "--config" | "-Z" | "-C" | "-j" | "--jobs" | "--manifest-path" => {
                words.get(index + 1)?;
                index += 2;
            }
            value
                if value.starts_with("--color=")
                    || value.starts_with("--config=")
                    || value.starts_with("--jobs=")
                    || value.starts_with("--manifest-path=")
                    || value.starts_with("-Z") && value.len() > 2
                    || value.starts_with("-C") && value.len() > 2
                    || value.starts_with("-j") && value.len() > 2
                    || value.starts_with('-')
                        && value.len() > 1
                        && value[1..].chars().all(|character| character == 'v') =>
            {
                index += 1;
            }
            value if !value.starts_with('-') => return Some(index),
            _ => return None,
        }
    }
    None
}

#[cfg(test)]
#[path = "invocation/label.rs"]
mod label;
#[cfg(test)]
#[path = "invocation/location.rs"]
mod location;
#[cfg(test)]
#[path = "invocation/preflight.rs"]
mod preflight;
#[cfg(test)]
#[path = "invocation/targets.rs"]
mod targets;
