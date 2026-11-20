// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::filesystem;
use super::manifest::{self, CandidateManifest};

pub(super) fn validate(manifest_path: &Path, workflow_root: &Path) -> Result<(), String> {
    let manifest = manifest::read(manifest_path)
        .map_err(|error| format!("cannot read candidate manifest: {error}"))?;
    let tree = filesystem::read_tree(workflow_root)?;
    let mut files = tree.files;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut count = 0usize;
    for (path, bytes) in files {
        let text = std::str::from_utf8(&bytes).map_err(|_| {
            format!(
                "cannot read generated workflow {}: invalid UTF-8",
                path.display()
            )
        })?;
        count += validate_file(&path, text, &manifest)?;
    }
    if count == 0 {
        return Err("no ConsumerV1 download bindings were emitted".into());
    }
    Ok(())
}

fn validate_file(path: &Path, text: &str, manifest: &CandidateManifest) -> Result<usize, String> {
    let lines: Vec<_> = text.lines().collect();
    let mut index = 0usize;
    let mut count = 0usize;
    while index < lines.len() {
        if !is_marker(lines[index]) {
            index += 1;
            continue;
        }
        let expected = [
            "VELNOR_ASSET_SHA256",
            "VELNOR_ASSET_URL",
            "VELNOR_RELEASE_COMMIT",
        ];
        if index + expected.len() > lines.len() {
            return Err(format!("incomplete binding in {}", path.display()));
        }
        let parsed: Vec<_> = lines[index..index + expected.len()]
            .iter()
            .map(|line| parse_line(line))
            .collect::<Result<_, _>>()
            .map_err(|()| format!("incomplete binding in {}", path.display()))?;
        if parsed.iter().map(|entry| entry.1).ne(expected) {
            return Err(format!("unpaired binding field in {}", path.display()));
        }
        if parsed.iter().any(|entry| entry.0 != parsed[0].0) {
            return Err(format!(
                "binding fields use different scopes in {}",
                path.display()
            ));
        }
        let Some(target) = manifest.by_url.get(parsed[1].2) else {
            return Err(format!("unknown release URL in {}", path.display()));
        };
        if parsed[0].2 != target.digest.as_str() {
            return Err(format!(
                "wrong digest for target {} in {}",
                target.target,
                path.display()
            ));
        }
        if parsed[2].2 != manifest.commit.as_str() {
            return Err(format!("wrong source commit in {}", path.display()));
        }
        count += 1;
        index += expected.len();
    }
    Ok(count)
}

fn is_marker(line: &str) -> bool {
    let trimmed = line.trim_start_matches([' ', '\t']);
    [
        "VELNOR_ASSET_SHA256",
        "VELNOR_ASSET_URL",
        "VELNOR_RELEASE_COMMIT",
    ]
    .iter()
    .any(|key| {
        trimmed
            .strip_prefix(key)
            .is_some_and(|tail| tail.starts_with([' ', '\t', ':']))
    })
}

fn parse_line(line: &str) -> Result<(&str, &str, &str), ()> {
    let indent_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    let indent = &line[..indent_len];
    let (key, value) = line[indent_len..].split_once(": ").ok_or(())?;
    if key.is_empty() || value.is_empty() || value.chars().any(char::is_whitespace) {
        return Err(());
    }
    Ok((indent, key, value))
}
