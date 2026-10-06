//! Parse rustc Makefile-style dependency prerequisites.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

type Outcome<T> = Result<T, Box<dyn Error>>;

pub(super) fn sources(path: &Path, workspace_root: &Path) -> Outcome<Vec<PathBuf>> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("cannot read rustc dep-info {}: {error}", path.display()))?;
    let mut logical_line = String::new();
    let mut characters = contents.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\n' {
            break;
        }
        if character == '\\' && matches!(characters.peek(), Some('\n' | '\r')) {
            if characters.next() == Some('\r') {
                characters.next_if_eq(&'\n');
            }
            logical_line.push(' ');
        } else if character != '\r' {
            logical_line.push(character);
        }
    }
    let (_, prerequisites) = logical_line
        .split_once(": ")
        .ok_or("rustc dep-info has no prerequisite list")?;
    make_words(prerequisites)?
        .into_iter()
        .map(|name| {
            let dependency = PathBuf::from(name);
            let resolved = if dependency.is_absolute() {
                dependency.clone()
            } else {
                workspace_root.join(&dependency)
            };
            resolved.canonicalize().map_err(|error| {
                format!(
                    "rustc dep-info {} references missing source {}: {error}",
                    path.display(),
                    resolved.display()
                )
                .into()
            })
        })
        .collect()
}

fn make_words(input: &str) -> Outcome<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut escaped = false;
    for character in input.chars() {
        if escaped {
            if character != '\n' && character != '\r' {
                word.push(character);
            }
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character.is_whitespace() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
        } else {
            word.push(character);
        }
    }
    if escaped {
        return Err("rustc dep-info ends with an incomplete escape".into());
    }
    if !word.is_empty() {
        words.push(word);
    }
    Ok(words)
}
