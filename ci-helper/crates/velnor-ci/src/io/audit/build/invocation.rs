// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use super::super::super::units::{MetadataInvocation, Workspace, workspace};
use crate::error::CiError;

pub(super) struct Invocation {
    pub(super) command: Vec<String>,
    pub(super) preflight: Option<Vec<String>>,
    pub(super) compiler: bool,
    pub(super) cargo: bool,
    globals: Vec<String>,
}

impl Invocation {
    pub(super) fn selects(&self, package: &str) -> bool {
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
        if !excluded
            && words
                .iter()
                .any(|word| matches!(*word, "--workspace" | "--all"))
        {
            return true;
        }
        words.iter().enumerate().any(|(index, word)| {
            (matches!(*word, "-p" | "--package") && words.get(index + 1) == Some(&package))
                || word.strip_prefix("--package=") == Some(package)
                || word.strip_prefix("-p") == Some(package)
        })
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
            let mut preflight = command.to_vec();
            let end = preflight
                .iter()
                .position(|word| word == "--")
                .unwrap_or(preflight.len());
            preflight.insert(end, "--no-run".to_owned());
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
        Self {
            command: command.to_vec(),
            preflight,
            compiler,
            cargo: compilation || nextest || test,
            globals,
        }
    }

    pub(super) fn location(&self, directory: &Path) -> PathBuf {
        if !self.cargo {
            return directory.to_path_buf();
        }
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
                || directory.to_path_buf(),
                |manifest| {
                    directory
                        .join(manifest)
                        .parent()
                        .unwrap_or(directory)
                        .to_path_buf()
                },
            )
    }

    pub(super) fn validate_targets(
        &self,
        directory: &Path,
        workspaces: &[Workspace],
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
        for expected in workspaces {
            let actual = workspace(
                &expected.workspace_root.join("Cargo.toml"),
                Some(&invocation),
            )?;
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

fn subcommand(words: &[&str]) -> Option<usize> {
    let mut index = usize::from(
        words
            .first()
            .is_some_and(|word| word.starts_with('+') && word.len() > 1),
    );
    while let Some(word) = words.get(index) {
        match *word {
            "-q" | "--quiet" | "--verbose" | "--locked" | "--offline" | "--frozen" => index += 1,
            "--color" | "--config" | "-Z" => {
                words.get(index + 1)?;
                index += 2;
            }
            value
                if value.starts_with("--color=")
                    || value.starts_with("--config=")
                    || value.starts_with("-Z") && value.len() > 2
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
