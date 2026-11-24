// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};
use std::path::Path;

use super::{Invocation, subcommand};
use crate::error::CiError;
use crate::io::crates_mirror_alias::expand;

pub(in super::super) fn resolve(
    command: &[String],
    directory: &Path,
) -> Result<Vec<String>, CiError> {
    if Path::new(&command[0]).file_name() != Some(OsStr::new("cargo")) {
        return Ok(command.to_vec());
    }
    let arguments: Vec<OsString> = command[1..].iter().map(OsString::from).collect();
    let expanded =
        expand(&Invocation::new(command).work(directory), &arguments).and_then(|expanded| {
            expanded
                .into_iter()
                .map(|word| word.into_string().ok())
                .collect::<Option<Vec<String>>>()
        });
    let words: Vec<&str> = command[1..].iter().map(String::as_str).collect();
    let name = subcommand(&words).map_or("", |index| words[index]);
    match expanded {
        Some(expanded) if shell_alias(&expanded, &words) => Err(CiError::new(format!(
            "cargo alias {name} runs a shell command; run the command it names outside inputs build"
        ))),
        Some(expanded) => {
            let separator = usize::from(expanded.first().is_some_and(|word| word == "--"));
            Ok(std::iter::once(command[0].clone())
                .chain(expanded.into_iter().skip(separator))
                .collect())
        }
        None => Err(CiError::new(format!(
            "cannot resolve the cargo alias {name}; run the cargo command it expands to"
        ))),
    }
}

fn shell_alias(expanded: &[String], words: &[&str]) -> bool {
    subcommand(words)
        .and_then(|index| expanded.get(index))
        .is_some_and(|word| word.starts_with('!'))
}
