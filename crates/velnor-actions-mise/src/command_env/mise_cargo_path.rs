//! Remove Mise Cargo command wrappers without disturbing selected Rustup paths.

use std::ffi::OsString;

/// Remove only the caller's canonical Mise Cargo wrappers and shims from the
/// last PATH assignment while preserving Rustup and unrelated path entries.
/// MBX invokes Cargo internally; exposing either Mise path makes
/// `mise exec ... -- cargo` resolve back to Mise instead of the Rustup-selected
/// Cargo binary.
pub(super) fn sanitize(env: &mut Vec<(OsString, OsString)>, parent: &[(OsString, OsString)]) {
    let Some(data_dir) = mise_data_dir(parent) else {
        return;
    };
    let rejected = [
        data_dir.join("command-wrappers").join("bin"),
        data_dir.join("shims"),
    ];
    let Some((_, path)) = env.iter().rev().find(|(key, _)| key == "PATH") else {
        return;
    };
    let entries = std::env::split_paths(path)
        .filter(|entry| !rejected.iter().any(|path| same_path(entry, path)))
        .collect::<Vec<_>>();
    let Ok(path) = std::env::join_paths(entries) else {
        // A malformed PATH must not reintroduce the rejected shim. Omitting
        // it makes MBX fail closed when it tries to resolve its compiler.
        env.retain(|(key, _)| key != "PATH");
        return;
    };
    env.retain(|(key, _)| key != "PATH");
    env.push((OsString::from("PATH"), path));
}

fn mise_data_dir(parent: &[(OsString, OsString)]) -> Option<std::path::PathBuf> {
    let mise_data_dir = parent
        .iter()
        .rev()
        .find(|(key, _)| key == "MISE_DATA_DIR")
        .map(|(_, value)| std::path::PathBuf::from(value));
    // Keep the default path tied to the caller's home rather than the
    // isolated MISE_DATA_DIR added for the child.
    let base = mise_data_dir.or_else(|| {
        parent
            .iter()
            .rev()
            .find(|(key, _)| key == "HOME")
            .map(|(_, home)| std::path::PathBuf::from(home).join(".local/share/mise"))
    })?;
    if !base.is_absolute() {
        return None;
    }
    Some(base)
}

fn same_path(left: &std::path::Path, right: &std::path::Path) -> bool {
    let left = left.components().collect::<Vec<_>>();
    let right = right.components().collect::<Vec<_>>();
    left.len() == right.len()
        && left.iter().zip(right.iter()).all(|(left, right)| {
            let left = left.as_os_str().to_string_lossy();
            let right = right.as_os_str().to_string_lossy();
            if cfg!(windows) || cfg!(target_os = "macos") {
                left.eq_ignore_ascii_case(&right)
            } else {
                left == right
            }
        })
}
