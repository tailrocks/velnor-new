//! Rust checkout probes: files, lockfile, configs, package walks.
//!
//! Lenient probes skip unreadable entries; strict probes fail closed.
//! Paths stay repository-relative; symlinks never resolve.

use std::path::Path;

use velnor_actions_contract::{Provenance, digest_b3};

use crate::identity::normalize_identity_path;

/// Schema-definition extensions collected for compiling kinds.
pub(crate) const SCHEMA_EXTS: [&str; 3] = ["proto", "graphql", "avsc"];

/// Provenance of one file: content digest, proven absence, or unknown.
pub(crate) fn probe_file(root: &Path, path: &str) -> Provenance {
    let Ok(normalized) = normalize_identity_path(path) else {
        return Provenance::Unknown {
            reason: format!("bad_path:{path}"),
        };
    };
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
        triage!(root, configured);
        if configured == CONVENTIONAL {
            return Provenance::AbsentProven {
                evidence: format!("not_found:{CONVENTIONAL}"),
            };
        }
    }
    match probe_file(root, CONVENTIONAL) {
        known @ Provenance::Known { .. } => known,
        unknown @ Provenance::Unknown { .. } => unknown,
        Provenance::AbsentProven { evidence } => Provenance::AbsentProven {
            evidence: format!("profile:{profile_config:?}:{evidence}"),
        },
        guarded @ Provenance::GuardedExternally { .. } => guarded,
    }
}

/// Cargo-config provenance: manifest dir plus the repository root.
pub(crate) fn probe_cargo_config(root: &Path, manifest: &str) -> Provenance {
    let dir = package_dir(manifest);
    let mut candidates = Vec::new();
    if !dir.is_empty() {
        candidates.push(format!("{dir}/.cargo/config.toml"));
    }
    candidates.push(".cargo/config.toml".to_owned());
    for candidate in &candidates {
        triage!(root, candidate);
    }
    Provenance::AbsentProven {
        evidence: format!("not_found:{}", candidates.join(",")),
    }
}

/// Declared-extra provenance: missing means unknown, never absent.
pub(crate) fn probe_declared(root: &Path, path: &str) -> Provenance {
    match probe_file(root, path) {
        Provenance::AbsentProven { evidence } => Provenance::Unknown {
            reason: format!("declared_but_missing:{evidence}"),
        },
        other => other,
    }
}

/// Directory of a manifest path; empty for the repository root.
fn package_dir(manifest: &str) -> &str {
    manifest.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Package `*.rs` files as sorted `(path, digest)` pairs (strict).
pub(crate) fn source_tree_files(
    root: &Path,
    manifest: &str,
) -> Result<Vec<(String, String)>, String> {
    let is_source = |path: &Path, _: &str| path.extension().is_some_and(|ext| ext == "rs");
    walk_full(
        root,
        manifest,
        &is_source,
        2000,
        "too_many_source_files",
        true,
    )
}

/// Package files accepted by `keep` as sorted pairs (lenient class walk).
pub(crate) fn walk_package_files(
    root: &Path,
    manifest: &str,
    keep: &dyn Fn(&Path, &str) -> bool,
) -> Result<Vec<(String, String)>, String> {
    walk_full(root, manifest, keep, 500, "too_many_class_files", false)
}

/// Walk the package dir skipping `target`/`.git`, collecting `(path, digest)`
/// pairs accepted by `keep`. Strict fails on symlinks/unreadable entries; lenient skips.
fn walk_full(
    root: &Path,
    manifest: &str,
    keep: &dyn Fn(&Path, &str) -> bool,
    cap: usize,
    cap_reason: &'static str,
    strict: bool,
) -> Result<Vec<(String, String)>, String> {
    let dir = package_dir(manifest);
    let mut base = root.to_path_buf();
    if !dir.is_empty() {
        base.push(dir);
    }
    let mut files = Vec::new();
    let mut stack = vec![base];
    while let Some(current) = stack.pop() {
        let entries = match std::fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(err) if strict => {
                return Err(format!("unreadable_dir:{}:{err}", current.display()));
            }
            Err(_) => return Err(format!("unreadable_dir:{}", current.display())),
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) if !strict => continue,
                Err(err) => return Err(format!("unreadable_entry:{}:{err}", current.display())),
            };
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) if !strict => return Err("unreadable_type".to_owned()),
                Err(err) => return Err(format!("unreadable_type:{}:{err}", current.display())),
            };
            if file_type.is_symlink() {
                if strict {
                    return Err(format!("symlink:{}", entry.path().display()));
                }
                continue;
            }
            if file_type.is_dir() {
                let name = entry.file_name();
                if name != "target" && name != ".git" {
                    stack.push(entry.path());
                }
                continue;
            }
            let full = entry.path();
            let Ok(rel) = full.strip_prefix(root) else {
                if strict {
                    return Err("outside_root".to_owned());
                }
                continue;
            };
            let rel = rel.to_string_lossy().replace('\\', "/");
            if !keep(&full, &rel) {
                continue;
            }
            match std::fs::read(&full) {
                Ok(bytes) => files.push((rel, digest_b3(&bytes))),
                Err(_) if !strict => {}
                Err(err) => return Err(format!("unreadable:{}:{err}", full.display())),
            }
            if files.len() > cap {
                return Err(cap_reason.to_owned());
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Whether `path` carries the `want` extension (ASCII case-insensitive).
pub(crate) fn ext_is(path: &str, want: &str) -> bool {
    Path::new(path)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(want))
}
