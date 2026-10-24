//! Native helpers used by the Velnor release qualification path.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
mod strict_json;
use strict_json::parse_strict_json_bytes;

const MAX_UNTRUSTED_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;

const MAX_CARGO_MESSAGES_BYTES: u64 = 64 * 1024 * 1024;
const MAX_CANDIDATE_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;
const MAX_WORKFLOW_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_WORKFLOW_TREE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_WORKFLOW_FILES: usize = 10_000;
const MAX_WORKFLOW_DEPTH: usize = 128;
const EXPECTED_TARGETS: [&str; 3] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
];

/// Resolve a path supplied to the private repository-policy operation.
fn rooted_path(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

mod cargo_artifact;
#[cfg(test)]
use cargo_artifact::cargo_artifact_executable;
pub(super) use cargo_artifact::print_cargo_artifact_executable;

pub(super) fn print_qualification_output_bindings(
    root: &Path,
    manifest_path: &Path,
    workflow_root: &Path,
) -> i32 {
    let manifest_path = rooted_path(root, manifest_path);
    let workflow_root = rooted_path(root, workflow_root);
    match validate_qualification_output_bindings(&manifest_path, &workflow_root) {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("candidate output binding mismatch: {error}");
            1
        }
    }
}

pub(super) fn print_qualification_manifest(root: &Path, manifest_path: &Path) -> i32 {
    let manifest_path = rooted_path(root, manifest_path);
    match validate_qualification_manifest(&manifest_path) {
        Ok(_) => 0,
        Err(error) => {
            eprintln!("candidate manifest validation: {error}");
            1
        }
    }
}

fn validate_qualification_manifest(manifest_path: &Path) -> Result<Value, String> {
    let bytes = read_bounded_regular_file(manifest_path, MAX_CANDIDATE_MANIFEST_BYTES)?;
    let manifest = parse_strict_json_bytes(
        &bytes,
        usize::try_from(MAX_CANDIDATE_MANIFEST_BYTES)
            .map_err(|_| "candidate manifest limit unavailable".to_owned())?,
    )
    .map_err(|error| format!("malformed or duplicate-key JSON: {error}"))?;
    if manifest.get("schema").and_then(Value::as_u64) != Some(1)
        || manifest.get("repository").and_then(Value::as_str) != Some("tailrocks/velnor-new")
    {
        return Err(
            "candidate manifest does not match the release schema or repository".to_owned(),
        );
    }
    let version = manifest
        .get("version")
        .and_then(Value::as_str)
        .filter(|version| is_release_version(version))
        .ok_or_else(|| "candidate manifest version is malformed".to_owned())?;
    let commit = manifest
        .get("commit")
        .and_then(Value::as_str)
        .filter(|value| is_lower_hex(value, 40))
        .ok_or_else(|| "candidate manifest source commit is malformed".to_owned())?;
    let _ = commit;
    let targets = manifest
        .get("targets")
        .and_then(Value::as_array)
        .filter(|targets| targets.len() == 3)
        .ok_or_else(|| "candidate manifest target inventory is malformed".to_owned())?;
    for (item, expected_target) in targets.iter().zip(EXPECTED_TARGETS) {
        let target = item.get("target").and_then(Value::as_str);
        let digest = item.get("sha256").and_then(Value::as_str);
        let artifact = item.get("artifact").and_then(Value::as_str);
        let expected_url = format!(
            "https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{expected_target}"
        );
        if target != Some(expected_target)
            || !digest.is_some_and(|value| is_lower_hex(value, 64))
            || artifact != Some(expected_url.as_str())
        {
            return Err(
                "candidate manifest does not match the release schema or target inventory"
                    .to_owned(),
            );
        }
    }
    Ok(manifest)
}

fn validate_qualification_output_bindings(
    manifest_path: &Path,
    workflow_root: &Path,
) -> Result<(), String> {
    let manifest = validate_qualification_manifest(manifest_path)
        .map_err(|error| format!("cannot read candidate manifest: {error}"))?;
    let commit = manifest
        .get("commit")
        .and_then(Value::as_str)
        .ok_or_else(|| "candidate manifest or generated workflow tree is incomplete".to_owned())?;
    let targets = manifest
        .get("targets")
        .and_then(Value::as_array)
        .ok_or_else(|| "candidate manifest or generated workflow tree is incomplete".to_owned())?;
    if targets.is_empty()
        || !workflow_root.is_dir()
        || fs::symlink_metadata(workflow_root)
            .map_err(|error| error.to_string())?
            .file_type()
            .is_symlink()
    {
        return Err("candidate manifest or generated workflow tree is incomplete".to_owned());
    }
    let mut by_url = std::collections::BTreeMap::new();
    for target in targets {
        let url = target
            .get("artifact")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                "candidate manifest or generated workflow tree is incomplete".to_owned()
            })?;
        let name = target
            .get("target")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                "candidate manifest or generated workflow tree is incomplete".to_owned()
            })?;
        let digest = target
            .get("sha256")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                "candidate manifest or generated workflow tree is incomplete".to_owned()
            })?;
        if by_url.insert(url, (name, digest)).is_some() {
            return Err("candidate manifest contains duplicate release URLs".to_owned());
        }
    }
    let mut files = Vec::new();
    collect_regular_files(workflow_root, &mut files, 0)?;
    files.sort();
    let total_bytes = files.iter().try_fold(0_u64, |total, path| {
        let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
        total
            .checked_add(metadata.len())
            .filter(|next| *next <= MAX_WORKFLOW_TREE_BYTES)
            .ok_or_else(|| {
                format!("generated workflow tree exceeds {MAX_WORKFLOW_TREE_BYTES} bytes")
            })
    })?;
    let _ = total_bytes;
    let mut bindings = 0usize;
    for path in files {
        let bytes = read_bounded_regular_file(&path, MAX_WORKFLOW_FILE_BYTES).map_err(|error| {
            format!("cannot read generated workflow {}: {error}", path.display())
        })?;
        let text = std::str::from_utf8(&bytes).map_err(|_| {
            format!(
                "cannot read generated workflow {}: invalid UTF-8",
                path.display()
            )
        })?;
        bindings += validate_file_bindings(path.as_path(), text, &by_url, commit)?;
    }
    if bindings == 0 {
        return Err("no ConsumerV1 download bindings were emitted".to_owned());
    }
    Ok(())
}

fn read_bounded_regular_file(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_file() || metadata.len() > limit {
        return Err(format!(
            "expected regular file no larger than {limit} bytes"
        ));
    }
    fs::read(path).map_err(|error| error.to_string())
}

fn collect_regular_files(
    root: &Path,
    files: &mut Vec<PathBuf>,
    depth: usize,
) -> Result<(), String> {
    if depth > MAX_WORKFLOW_DEPTH {
        return Err(format!(
            "generated workflow tree exceeds {MAX_WORKFLOW_DEPTH} directory levels"
        ));
    }
    let mut entries: Vec<_> = fs::read_dir(root)
        .map_err(|error| error.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let kind = fs::symlink_metadata(&path)
            .map_err(|error| error.to_string())?
            .file_type();
        if kind.is_symlink() {
            continue;
        }
        if kind.is_dir() {
            collect_regular_files(&path, files, depth + 1)?;
        } else if kind.is_file() {
            if files.len() == MAX_WORKFLOW_FILES {
                return Err(format!(
                    "generated workflow tree exceeds {MAX_WORKFLOW_FILES} regular files"
                ));
            }
            files.push(path);
        }
    }
    Ok(())
}

fn is_binding_marker(line: &str) -> bool {
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

fn parse_binding_line(line: &str) -> Result<(&str, &str, &str), ()> {
    let indent_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    let indent = &line[..indent_len];
    let body = &line[indent_len..];
    let (key, value) = body.split_once(": ").ok_or(())?;
    if key.is_empty() || value.is_empty() || value.chars().any(char::is_whitespace) {
        return Err(());
    }
    Ok((indent, key, value))
}

fn is_release_version(value: &str) -> bool {
    let parts: Vec<_> = value.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
}

fn is_lower_hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[path = "release_support_tests.rs"]
mod tests;

fn validate_file_bindings(
    path: &Path,
    text: &str,
    by_url: &std::collections::BTreeMap<&str, (&str, &str)>,
    commit: &str,
) -> Result<usize, String> {
    let mut bindings = 0usize;
    let lines: Vec<&str> = text.lines().collect();
    let mut index = 0;
    while index < lines.len() {
        if !is_binding_marker(lines[index]) {
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
            .map(|line| parse_binding_line(line))
            .collect::<Result<_, _>>()
            .map_err(|()| format!("incomplete binding in {}", path.display()))?;
        if parsed.iter().map(|item| item.1).ne(expected) {
            return Err(format!("unpaired binding field in {}", path.display()));
        }
        if parsed.iter().any(|item| item.0 != parsed[0].0) {
            return Err(format!(
                "binding fields use different scopes in {}",
                path.display()
            ));
        }
        let digest = parsed[0].2;
        let url = parsed[1].2;
        let source = parsed[2].2;
        let Some((target_name, expected_digest)) = by_url.get(url) else {
            return Err(format!("unknown release URL in {}", path.display()));
        };
        if digest != *expected_digest {
            return Err(format!(
                "wrong digest for target {target_name} in {}",
                path.display()
            ));
        }
        if source != commit {
            return Err(format!("wrong source commit in {}", path.display()));
        }
        bindings += 1;
        index += expected.len();
    }
    Ok(bindings)
}
