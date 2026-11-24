// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::ffi::{OsStr, OsString};
use std::path::Path;

use super::{CARGO_SUBCOMMANDS, Invocation, subcommand};
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
    if let Some(words) = expanded {
        return Ok(std::iter::once(command[0].clone()).chain(words).collect());
    }
    let words: Vec<&str> = command[1..].iter().map(String::as_str).collect();
    match subcommand(&words).map(|index| words[index]) {
        Some(name) if !CARGO_SUBCOMMANDS.contains(&name) && name != "nextest" => Err(CiError::new(
            format!("cannot resolve the cargo alias {name}; run the cargo command it expands to"),
        )),
        _ => Ok(command.to_vec()),
    }
}
