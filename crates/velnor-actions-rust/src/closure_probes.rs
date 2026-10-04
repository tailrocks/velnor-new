//! Rust checkout probes: files, lockfile, configs, package walks.
//!
//! Lenient probes skip unreadable entries; strict probes fail closed.
//! Paths stay repository-relative; symlinks never resolve.

use std::path::Path;

use velnor_actions_contract::{Provenance, digest_b3};

use crate::identity::normalize_identity_path;

/// Provenance of one file: content digest, proven absence, or unknown.
pub(crate) fn probe_file(root: &Path, path: &str) -> Provenance {
    let Ok(normalized) = normalize_identity_path(path) else {
        return Provenance::Unknown {
            reason: format!("bad_path:{path}"),
        };
    };
    let mut checked = root.to_path_buf();
    for component in Path::new(&normalized).components() {
        checked.push(component);
        if std::fs::symlink_metadata(&checked).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Provenance::Unknown {
                reason: format!("symlink:{normalized}"),
            };
        }
    }
    match std::fs::read(root.join(&normalized)) {
        Ok(bytes) => Provenance::Known {
            digest: digest_b3(&bytes),
        },
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Provenance::AbsentProven {
            evidence: format!("not_found:{normalized}"),
        },
        Err(err) => Provenance::Unknown {
            reason: format!("unreadable:{normalized}:{err}"),
        },
    }
}

/// Decisive probes win; absence and guards fall through to the next probe.
macro_rules! triage {
    ($root:expr, $candidate:expr) => {
        match probe_file($root, $candidate) {
            known @ Provenance::Known { .. } => return known,
            unknown @ Provenance::Unknown { .. } => return unknown,
            Provenance::AbsentProven { .. } | Provenance::GuardedExternally { .. } => {}
        }
    };
}

/// Lockfile provenance, walking up from the manifest like Cargo does.
pub(crate) fn probe_lockfile(root: &Path, manifest: &str) -> Provenance {
    let mut dir = package_dir(manifest).to_owned();
    let mut probed = Vec::new();
    loop {
        let candidate = if dir.is_empty() {
            "Cargo.lock".to_owned()
        } else {
            format!("{dir}/Cargo.lock")
        };
        triage!(root, &candidate);
        probed.push(candidate);
        match dir.rsplit_once('/') {
            Some((parent, _)) => dir = parent.to_owned(),
            None if dir.is_empty() => break,
            None => dir.clear(),
        }
    }
    Provenance::AbsentProven {
        evidence: format!("not_found:{}", probed.join(",")),
    }
}

/// Nextest-config provenance: profile path plus the conventional path.
pub(crate) fn probe_nextest_config(root: &Path, profile_config: Option<&str>) -> Provenance {
    const CONVENTIONAL: &str = ".config/nextest.toml";
    if let Some(configured) = profile_config {
        match probe_nextest_file(root, configured) {
            known @ Provenance::Known { .. } => return known,
            unknown @ Provenance::Unknown { .. } => return unknown,
            Provenance::AbsentProven { .. } | Provenance::GuardedExternally { .. } => {}
        }
        if configured == CONVENTIONAL {
            return Provenance::AbsentProven {
                evidence: format!("not_found:{CONVENTIONAL}"),
            };
        }
    }
    match probe_nextest_file(root, CONVENTIONAL) {
        known @ Provenance::Known { .. } => known,
        unknown @ Provenance::Unknown { .. } => unknown,
        Provenance::AbsentProven { evidence } => Provenance::AbsentProven {
            evidence: format!("profile:{profile_config:?}:{evidence}"),
        },
        guarded @ Provenance::GuardedExternally { .. } => guarded,
    }
}

/// Only self-contained scalar profile settings currently prove config completeness.
fn probe_nextest_file(root: &Path, path: &str) -> Provenance {
    let provenance = probe_file(root, path);
    if !matches!(provenance, Provenance::Known { .. }) {
        return provenance;
    }
    let known = std::fs::read_to_string(root.join(path))
        .ok()
        .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
        .is_some_and(|value| simple_nextest_config(&value));
    if known {
        provenance
    } else {
        Provenance::Unknown {
            reason: format!("nextest_config_consumed_inputs_unproven:{path}"),
        }
    }
}

/// A closed safe subset: no scripts, wrappers, dynamic hooks, or external inputs.
fn simple_nextest_config(value: &toml::Value) -> bool {
    let Some(root) = value.as_table() else {
        return false;
    };
    if root.keys().any(|key| key != "profile") {
        return false;
    }
    let Some(profiles) = root.get("profile").and_then(toml::Value::as_table) else {
        return root.is_empty();
    };
    let allowed = [
        "retries",
        "test-threads",
        "status-level",
        "final-status-level",
        "failure-output",
        "success-output",
        "fail-fast",
    ];
    profiles.values().all(|profile| {
        profile.as_table().is_some_and(|settings| {
            settings.iter().all(|(key, value)| {
                allowed.contains(&key.as_str())
                    && (value.is_str() || value.is_integer() || value.is_bool())
            })
        })
    })
}

/// Cargo config merges all ancestor levels; bind every supported filename.
pub(crate) fn probe_cargo_config(root: &Path, manifest: &str) -> Provenance {
    let mut dir = package_dir(manifest).to_owned();
    let mut candidates = Vec::new();
    loop {
        for name in ["config", "config.toml"] {
            candidates.push(if dir.is_empty() {
                format!(".cargo/{name}")
            } else {
                format!("{dir}/.cargo/{name}")
            });
        }
        match dir.rsplit_once('/') {
            Some((parent, _)) => dir = parent.to_owned(),
            None if dir.is_empty() => break,
            None => dir.clear(),
        }
    }
    let mut files = Vec::new();
    for path in &candidates {
        match probe_file(root, path) {
            Provenance::Known { digest } => files.push((path, digest)),
            unknown @ Provenance::Unknown { .. } => return unknown,
            Provenance::AbsentProven { .. } | Provenance::GuardedExternally { .. } => {}
        }
    }
    if files.is_empty() {
        return Provenance::AbsentProven {
            evidence: format!("not_found:{}", candidates.join(",")),
        };
    }
    match velnor_actions_contract::canonical_json_bytes(&files) {
        Ok(bytes) => Provenance::Unknown {
            reason: format!(
                "cargo_config_consumed_inputs_unproven:{}",
                digest_b3(&bytes)
            ),
        },
        Err(err) => Provenance::Unknown {
            reason: err.to_string(),
        },
    }
}

/// Declared-extra provenance: missing means unknown, never absent.
pub(crate) fn probe_declared(root: &Path, path: &str) -> Provenance {
    match probe_file(root, path) {
        Provenance::AbsentProven { evidence } => Provenance::Unknown {
            reason: format!("declared_but_missing:{evidence}"),
        },
        Provenance::Known { digest } => declared_permissions(root, path, &digest),
        other => other,
    }
}

/// Declared executable fixtures must bind permissions as well as bytes.
fn declared_permissions(root: &Path, path: &str, digest: &str) -> Provenance {
    let metadata = match std::fs::symlink_metadata(root.join(path)) {
        Ok(metadata) => metadata,
        Err(err) => {
            return Provenance::Unknown {
                reason: err.to_string(),
            };
        }
    };
    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o777
    };
    #[cfg(not(unix))]
    let permissions = u32::from(metadata.permissions().readonly());
    match velnor_actions_contract::canonical_json_bytes(&(digest, permissions)) {
        Ok(bytes) => Provenance::Known {
            digest: digest_b3(&bytes),
        },
        Err(err) => Provenance::Unknown {
            reason: err.to_string(),
        },
    }
}

/// Directory of a manifest path; empty for the repository root.
fn package_dir(manifest: &str) -> &str {
    manifest.rsplit_once('/').map_or("", |(dir, _)| dir)
}

#[cfg(test)]
#[path = "../tests/closure_probe_tests.rs"]
mod tests;
