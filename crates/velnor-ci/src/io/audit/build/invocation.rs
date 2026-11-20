// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::super::super::units::{MetadataInvocation, Workspace, refuse_arguments};
use super::super::groups::root_workspace;
use crate::error::CiError;

#[derive(Default, Serialize, Deserialize)]
pub(super) struct Selection {
    all: bool,
    packages: Vec<String>,
}

impl Selection {
    pub(super) fn selects(&self, package: &str) -> bool {
        self.all || self.packages.iter().any(|selected| selected == package)
    }
}

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

    pub(super) fn selection(&self, members: &BTreeSet<&str>) -> Option<Selection> {
        if !self.cargo_program() {
            return None;
        }
        let words: Vec<&str> = self
            .command
            .iter()
            .skip(1)
            .map(String::as_str)
            .take_while(|word| *word != "--")
            .collect();
        let excluded = words
            .iter()
            .any(|word| *word == "--exclude" || word.starts_with("--exclude="));
        let all = !excluded
            && words
                .iter()
                .any(|word| matches!(*word, "--workspace" | "--all"));
        let mut packages = Vec::new();
        for (index, word) in words.iter().enumerate() {
            if matches!(*word, "-p" | "--package") {
                packages.extend(words.get(index + 1).map(|name| (*name).to_owned()));
            } else if let Some(name) = word
                .strip_prefix("--package=")
                .or_else(|| word.strip_prefix("-p"))
            {
                packages.push(name.to_owned());
            }
        }
        packages.retain(|name| members.contains(name.as_str()));
        Some(Selection { all, packages })
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
        let test = matches!((program, arguments), ("cargo", ["test" | "t", ..]));
        let doctest = test && words.contains(&"--doc");
        let compilation = matches!(
            (program, arguments),
            (
                "cargo",
                ["build" | "b" | "check" | "c" | "clippy" | "doc" | "d", ..]
            ) | ("cargo-clippy", ["clippy", ..])
                | ("cargo" | "cargo-nextest", ["nextest", "archive", ..])
        );
        let compiler = compilation && !words.contains(&"--open")
            || (nextest || test && !doctest) && words.contains(&"--no-run");
        let preflight = ((nextest || test && !doctest) && !compiler).then(|| {
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
        });
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
        Self {
            command: command.to_vec(),
            preflight,
            compiler,
            cargo: compilation || nextest || test,
            globals,
            chdir,
        }
    }

    pub(super) fn location(&self, directory: &Path) -> PathBuf {
        if !self.cargo {
            return directory.to_path_buf();
        }
        let base = self
            .chdir
            .as_ref()
            .map_or_else(|| directory.to_path_buf(), |chdir| directory.join(chdir));
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
        if !self.cargo {
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
        for ((expected, alone), inherit) in workspaces.iter().zip(standalone).zip(inherit) {
            let manifest = expected.workspace_root.join("Cargo.toml");
            let actual = root_workspace(&manifest, Some(&invocation), *inherit, *alone, groups)?;
            if actual.workspace_root != expected.workspace_root
                || actual.target_directory != expected.target_directory
            {
                return Err(CiError::new(
                    "Cargo invocation configuration redirects the workspace or target away from inputs audit; use persistent Cargo configuration or CARGO_TARGET_DIR for both build and audit",
                ));
            }
        }
        Ok(())
    }
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
#[path = "invocation/location.rs"]
mod location;
#[cfg(test)]
#[path = "invocation/preflight.rs"]
mod preflight;
