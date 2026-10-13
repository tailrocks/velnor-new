//! Parse rustc Makefile prerequisites and environment dependencies.

use std::collections::HashSet;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

type Outcome<T> = Result<T, Box<dyn Error>>;

pub(super) struct CompilerEvidence {
    pub(super) prerequisites: HashSet<PathBuf>,
    pub(super) out_dir: Option<PathBuf>,
}

pub(super) fn parse(path: &Path, workspace_root: &Path) -> Outcome<CompilerEvidence> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("cannot read rustc dep-info {}: {error}", path.display()))?;
    let prerequisites = prerequisites(&contents, path, workspace_root)?;
    let out_dir = out_dir_binding(&contents)?;
    Ok(CompilerEvidence {
        prerequisites,
        out_dir,
    })
}

fn prerequisites(contents: &str, path: &Path, workspace_root: &Path) -> Outcome<HashSet<PathBuf>> {
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
                dependency
            } else {
                workspace_root.join(dependency)
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

fn out_dir_binding(contents: &str) -> Outcome<Option<PathBuf>> {
    let mut binding = None;
    for line in contents.lines() {
        let Some(entry) = line.strip_prefix("# env-dep:") else {
            continue;
        };
        let Some((name, value)) = entry.split_once('=') else {
            if malformed_out_dir_name(entry) {
                return Err("rustc dep-info has a malformed OUT_DIR environment dependency".into());
            }
            continue;
        };
        if name != "OUT_DIR" {
            if malformed_out_dir_name(name) {
                return Err("rustc dep-info has a malformed OUT_DIR environment dependency".into());
            }
            continue;
        }
        if binding.is_some() {
            return Err("rustc dep-info has duplicate OUT_DIR environment dependencies".into());
        }
        if value.is_empty() || value.contains(['\r', '\n', '\0']) {
            return Err("rustc dep-info has an empty or malformed OUT_DIR value".into());
        }
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err("rustc dep-info OUT_DIR value is not absolute".into());
        }
        binding = Some(path);
    }
    Ok(binding)
}

fn malformed_out_dir_name(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("OUT_DIR") else {
        return false;
    };
    rest.is_empty()
        || rest
            .chars()
            .next()
            .is_some_and(|character| !character.is_ascii_alphanumeric() && character != '_')
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

#[cfg(test)]
mod tests {
    use super::out_dir_binding;
    use std::error::Error;
    use std::path::Path;

    type Outcome<T> = Result<T, Box<dyn Error>>;

    #[test]
    fn raw_out_dir_binding_preserves_spaces_and_ignores_other_variables() -> Outcome<()> {
        let parsed = out_dir_binding(
            "target: source.rs\n# env-dep:PROFILE=debug\n# env-dep:OUT_DIR=/tmp/cache path/out\n",
        )?;
        assert_eq!(parsed.as_deref(), Some(Path::new("/tmp/cache path/out")));
        assert_eq!(
            out_dir_binding("target: source.rs\n# env-dep:PROFILE=debug\n")?,
            None
        );
        Ok(())
    }

    #[test]
    fn out_dir_binding_rejects_malformed_duplicate_and_relative_values() -> Outcome<()> {
        for contents in [
            "target: source.rs\n# env-dep:OUT_DIR\n",
            "target: source.rs\n# env-dep:OUT_DIR=\n",
            "target: source.rs\n# env-dep:OUT_DIR=relative/out\n",
            "target: source.rs\n# env-dep:OUT_DIR=/tmp/one\n# env-dep:OUT_DIR=/tmp/one\n",
            "target: source.rs\n# env-dep:OUT_DIR=/tmp/one\n# env-dep:OUT_DIR=/tmp/two\n",
            "target: source.rs\n# env-dep:OUT_DIR=/tmp/out\0dir\n",
            "target: source.rs\n# env-dep:OUT_DIR ?= /tmp/out\n",
        ] {
            assert!(out_dir_binding(contents).is_err(), "accepted {contents:?}");
        }
        assert_eq!(
            out_dir_binding("target: source.rs\n# env-dep:OUT_DIR_ALIAS=/tmp/other\n")?,
            None
        );
        Ok(())
    }
}
