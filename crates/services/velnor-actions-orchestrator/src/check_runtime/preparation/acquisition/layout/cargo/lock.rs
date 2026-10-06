//! Cargo.lock identity and exhaustive crates.io archive admission.
use crate::OrchestratorError;
use crate::internal::internal;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;
use velnor_actions_contract_config::config::QualifiedToolPlatform;
use velnor_actions_mise::CheckDeadline;

#[derive(Deserialize)]
struct Manifest {
    package: PackageIdentity,
}
#[derive(Deserialize)]
struct PackageIdentity {
    name: String,
    version: String,
}
#[derive(Deserialize)]
struct CargoLock {
    version: u32,
    package: Vec<LockedPackage>,
}
#[derive(Deserialize)]
struct LockedPackage {
    name: String,
    version: String,
    source: Option<String>,
    checksum: Option<String>,
}

fn read(path: &Path, deadline: CheckDeadline) -> Result<Vec<u8>, OrchestratorError> {
    crate::retrieve_reports::staged_reads::read_staged_bytes_until(path, 4 * 1024 * 1024, || {
        deadline.remaining().map(|_| ()).map_err(|_| "deadline")
    })
    .map_err(|problem| {
        if problem == "deadline" {
            internal("check_timeout:deadline_exhausted")
        } else {
            internal("qualified_cargo_manifest_unreadable")
        }
    })
}

pub(super) fn package_identity(
    root: &Path,
    deadline: CheckDeadline,
) -> Result<(String, String), OrchestratorError> {
    check_deadline(deadline)?;
    let bytes = read(&root.join("Cargo.toml"), deadline)?;
    let text =
        std::str::from_utf8(&bytes).map_err(|_| internal("qualified_cargo_manifest_utf8"))?;
    let manifest: Manifest =
        toml::from_str(text).map_err(|_| internal("qualified_cargo_manifest_decode"))?;
    check_deadline(deadline)?;
    Ok((manifest.package.name, manifest.package.version))
}

pub(super) fn archive_identity(url: &str) -> Result<(String, String), OrchestratorError> {
    let rest = url
        .strip_prefix("https://")
        .ok_or_else(|| internal("qualified_cargo_archive_url"))?;
    let (host, path) = rest
        .split_once('/')
        .ok_or_else(|| internal("qualified_cargo_archive_url"))?;
    let components: Vec<_> = path.split('/').collect();
    match (host, components.as_slice()) {
        ("crates.io", ["api", "v1", "crates", name, version, "download"]) => {
            Ok(((*name).to_owned(), (*version).to_owned()))
        }
        ("static.crates.io", ["crates", name, filename]) => {
            let version = filename
                .strip_prefix(&format!("{name}-"))
                .and_then(|name| name.strip_suffix(".crate"))
                .ok_or_else(|| internal("qualified_cargo_archive_url"))?;
            Ok(((*name).to_owned(), version.to_owned()))
        }
        _ => Err(internal("qualified_cargo_archive_url")),
    }
}

pub(super) fn verify_closure(
    root: &Path,
    expected_sha: &str,
    platform: &QualifiedToolPlatform,
    primary: &(String, String),
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    check_deadline(deadline)?;
    let bytes = read(&root.join("Cargo.lock"), deadline)?;
    if sha256_with_deadline(&bytes, deadline)? != expected_sha {
        return Err(internal("qualified_cargo_lock_sha256"));
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| internal("qualified_cargo_lock_utf8"))?;
    let lock: CargoLock =
        toml::from_str(text).map_err(|_| internal("qualified_cargo_lock_decode"))?;
    if !(3..=4).contains(&lock.version) {
        return Err(internal("qualified_cargo_lock_version"));
    }
    let mut declared = BTreeMap::new();
    for artifact in &platform.dependency_artifacts {
        check_deadline(deadline)?;
        let identity = archive_identity(&artifact.url)?;
        if declared.insert(identity, artifact.sha256.clone()).is_some() {
            return Err(internal("qualified_cargo_duplicate_archive"));
        }
    }
    let mut observed = BTreeMap::new();
    let mut root_found = false;
    for package in lock.package {
        check_deadline(deadline)?;
        let identity = (package.name, package.version);
        match package.source.as_deref() {
            None if identity == *primary && package.checksum.is_none() && !root_found => {
                root_found = true;
            }
            Some(
                "registry+https://github.com/rust-lang/crates.io-index"
                | "registry+sparse+https://index.crates.io/",
            ) => {
                let checksum = package
                    .checksum
                    .ok_or_else(|| internal("qualified_cargo_missing_package_checksum"))?;
                if observed.insert(identity, checksum).is_some() {
                    return Err(internal("qualified_cargo_duplicate_lock_package"));
                }
            }
            _ => return Err(internal("qualified_cargo_unqualified_lock_source")),
        }
    }
    if !root_found || observed != declared {
        return Err(internal("qualified_cargo_dependency_closure"));
    }
    check_deadline(deadline)
}

fn sha256_with_deadline(
    bytes: &[u8],
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    let mut hash = Sha256::new();
    for chunk in bytes.chunks(64 * 1024) {
        check_deadline(deadline)?;
        hash.update(chunk);
    }
    let digest = hash.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        write!(&mut hex, "{byte:02x}").map_err(|_| internal("qualified_cargo_sha256"))?;
    }
    Ok(hex)
}

fn check_deadline(deadline: CheckDeadline) -> Result<(), OrchestratorError> {
    deadline
        .remaining()
        .map(|_| ())
        .map_err(|error| internal(&error.to_string()))
}
