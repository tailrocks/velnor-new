// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    MAX_CARGO_MESSAGES_BYTES, MAX_UNTRUSTED_DOCUMENT_BYTES, Path, PathBuf, Value, fs,
    parse_strict_json_bytes, rooted_path,
};

pub(crate) fn print_cargo_artifact_executable(
    root: &Path,
    messages_path: &Path,
    manifest_path: &Path,
    target_name: &str,
) -> i32 {
    match cargo_artifact_executable(
        root,
        &rooted_path(root, messages_path),
        &rooted_path(root, manifest_path),
        target_name,
    ) {
        Ok(path) => {
            println!("{}", path.display());
            0
        }
        Err(error) => {
            eprintln!("cargo artifact selection: {error}");
            1
        }
    }
}

pub(super) fn cargo_artifact_executable(
    workspace_root: &Path,
    messages_path: &Path,
    manifest_path: &Path,
    target_name: &str,
) -> Result<PathBuf, String> {
    let root = fs::canonicalize(workspace_root)
        .map_err(|error| format!("workspace root does not exist: {error}"))?;
    let manifest = fs::canonicalize(manifest_path).map_err(|error| {
        format!(
            "manifest does not exist: {}: {error}",
            manifest_path.display()
        )
    })?;
    let metadata = fs::metadata(messages_path)
        .map_err(|error| format!("cannot read Cargo messages: {error}"))?;
    if !metadata.is_file() || metadata.len() > MAX_CARGO_MESSAGES_BYTES {
        return Err(format!(
            "Cargo messages must be a regular file no larger than {MAX_CARGO_MESSAGES_BYTES} bytes"
        ));
    }
    let bytes =
        fs::read(messages_path).map_err(|error| format!("cannot read Cargo messages: {error}"))?;
    let text =
        std::str::from_utf8(&bytes).map_err(|_| "Cargo messages are not UTF-8".to_owned())?;
    let mut matches = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let record = parse_strict_json_bytes(line.as_bytes(), MAX_UNTRUSTED_DOCUMENT_BYTES)
            .map_err(|_| format!("invalid Cargo JSON at line {}", index + 1))?;
        if record.get("reason").and_then(Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        let Some(target) = record.get("target") else {
            continue;
        };
        if target.get("name").and_then(Value::as_str) != Some(target_name)
            || !target
                .get("kind")
                .and_then(Value::as_array)
                .is_some_and(|kinds| kinds.iter().any(|kind| kind.as_str() == Some("bin")))
        {
            continue;
        }
        let Some(emitted_manifest) = record.get("manifest_path").and_then(Value::as_str) else {
            continue;
        };
        let Ok(emitted_manifest) = fs::canonicalize(emitted_manifest) else {
            continue;
        };
        if emitted_manifest != manifest {
            continue;
        }
        let executable = resolve_executable(&record, target_name, &root)?;
        matches.push(executable);
    }
    if matches.len() != 1 {
        return Err(format!(
            "expected one Cargo executable for {target_name}, found {}",
            matches.len()
        ));
    }
    Ok(matches.remove(0))
}

#[cfg(unix)]
fn is_executable(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_metadata: &fs::Metadata) -> bool {
    true
}

fn resolve_executable(record: &Value, target_name: &str, root: &Path) -> Result<PathBuf, String> {
    let executable = record
        .get("executable")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("Cargo reported no executable for {target_name}"))?;
    let executable = Path::new(executable);
    let executable = if executable.is_absolute() {
        executable.to_path_buf()
    } else {
        root.join(executable)
    };
    let executable = fs::canonicalize(&executable)
        .map_err(|_| format!("Cargo executable does not exist: {}", executable.display()))?;
    let metadata = fs::metadata(&executable).map_err(|error| error.to_string())?;
    if !metadata.is_file() || !is_executable(&metadata) {
        return Err(format!(
            "Cargo executable is not executable: {}",
            executable.display()
        ));
    }
    Ok(executable)
}
