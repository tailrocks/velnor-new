// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

pub(super) struct Invocation {
    pub(super) command: Vec<String>,
    pub(super) preflight: Option<Vec<String>>,
    pub(super) compiler: bool,
    pub(super) cargo: bool,
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
        let nextest = matches!(
            (program, words.as_slice()),
            ("cargo" | "cargo-nextest", ["nextest", "run", ..])
        );
        let test = matches!((program, words.as_slice()), ("cargo", ["test", ..]));
        let doctest = test && words.contains(&"--doc");
        let compilation = matches!(
            (program, words.as_slice()),
            ("cargo", ["build" | "check" | "clippy" | "doc", ..])
                | ("cargo-clippy", ["clippy", ..])
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
        Self {
            command: command.to_vec(),
            preflight,
            compiler,
            cargo: compilation || nextest || test,
        }
    }

    pub(super) fn location(&self, directory: &Path) -> PathBuf {
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
}
