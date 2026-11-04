// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{cmp::Ordering, ffi::OsString, io::Write as _, path::Path, time::Duration};

use crate::{
    OrchestratorError,
    crate_registry_inventory::{CrateInventoryError, inspect_crate_versions},
    crate_release_controller::CrateReleaseAdmissionReceipt,
    crate_release_registry::CrateRegistryRead,
    crate_release_source::{source_cargo_path, source_compiler_env, source_tool_catalog},
    internal::internal,
};
use velnor_actions_mise::{PinnedTool, PinnedToolExec};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case", deny_unknown_fields)]
pub enum SemverEvidence {
    Checked {
        baseline: String,
        baseline_checksum: String,
        checker_sha256: String,
    },
    NoBaseline {
        checker_sha256: String,
    },
    Deferred {
        predecessors: Vec<String>,
    },
    PriorNoBaselinePending {
        checker_sha256: String,
        target_checksum: String,
    },
}

pub(crate) struct VerifiedSemver {
    package: String,
    version: String,
    source_sha: String,
    intent_sha256: String,
    evidence: SemverEvidence,
}

impl VerifiedSemver {
    pub(crate) fn into_bound_evidence(
        self,
        package: &str,
        version: &str,
        source_sha: &str,
        intent_sha256: &str,
    ) -> Result<SemverEvidence, OrchestratorError> {
        if self.package != package
            || self.version != version
            || self.source_sha != source_sha
            || self.intent_sha256 != intent_sha256
            || !self.evidence.is_final()
        {
            return Err(invalid());
        }
        if let SemverEvidence::Checked { baseline, .. } = &self.evidence {
            let baseline = semver::Version::parse(baseline).map_err(|_| invalid())?;
            let target = semver::Version::parse(version).map_err(|_| invalid())?;
            if baseline.cmp_precedence(&target) != Ordering::Less {
                return Err(invalid());
            }
        }
        Ok(self.evidence)
    }
}

impl SemverEvidence {
    pub(crate) fn valid_for(&self, version: &str) -> bool {
        if !self.is_final() {
            return false;
        }
        match self {
            Self::Checked { baseline, .. } => semver::Version::parse(baseline)
                .ok()
                .zip(semver::Version::parse(version).ok())
                .is_some_and(|(baseline, target)| {
                    baseline.cmp_precedence(&target) == Ordering::Less
                }),
            Self::NoBaseline { .. } => semver::Version::parse(version).is_ok(),
            Self::Deferred { .. } | Self::PriorNoBaselinePending { .. } => false,
        }
    }

    pub(crate) fn is_final(&self) -> bool {
        let Some(expected) =
            velnor_actions_mise::catalog::qualified_cargo_semver_checks("ubuntu-24.04")
        else {
            return false;
        };
        match self {
            Self::Checked {
                baseline,
                baseline_checksum,
                checker_sha256,
            } => {
                semver::Version::parse(baseline).is_ok()
                    && baseline_checksum.len() == 64
                    && baseline_checksum
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    && checker_sha256 == expected.executable_sha256
            }
            Self::NoBaseline { checker_sha256 } => checker_sha256 == expected.executable_sha256,
            Self::Deferred { .. } | Self::PriorNoBaselinePending { .. } => false,
        }
    }
}

pub(crate) fn matches_revalidated_preflight(
    prior: &std::collections::BTreeMap<String, SemverEvidence>,
    fresh: &std::collections::BTreeMap<String, SemverEvidence>,
) -> bool {
    prior.len() == fresh.len()
        && prior.iter().all(|(name, old)| {
            fresh.get(name).is_some_and(|new| {
                old == new
                    || matches!(
                        (old, new),
                        (
                            SemverEvidence::Deferred { .. },
                            SemverEvidence::Checked { .. } | SemverEvidence::NoBaseline { .. }
                        )
                    )
            })
        })
}

fn invalid() -> OrchestratorError {
    internal("crate_release_semver_unverified")
}

#[derive(Debug, PartialEq, Eq)]
struct Baseline {
    version: String,
    id: u64,
}

#[derive(serde::Deserialize)]
struct BaselineReply {
    version: BaselineVersion,
}

#[derive(serde::Deserialize)]
struct BaselineVersion {
    id: u64,
    #[serde(rename = "crate")]
    package: String,
    num: String,
    yanked: bool,
    checksum: String,
}

fn baseline<R: CrateRegistryRead>(
    registry: &R,
    package: &str,
    version: &str,
) -> Result<Option<Baseline>, OrchestratorError> {
    let target = semver::Version::parse(version).map_err(|_| invalid())?;
    let inventory = match inspect_crate_versions(registry, package) {
        Ok(inventory) => inventory,
        Err(CrateInventoryError::NotFound) => return Ok(None),
        Err(_) => return Err(invalid()),
    };
    let versions: Vec<_> = inventory
        .versions
        .into_iter()
        .filter(|(_, entry)| !entry.yanked)
        .collect();
    if versions.is_empty() {
        return Err(invalid());
    }
    let mut earlier = Vec::new();
    for (value, entry) in versions {
        let parsed = semver::Version::parse(&value).map_err(|_| invalid())?;
        if parsed.cmp_precedence(&target) == Ordering::Less {
            earlier.push((parsed, value, entry.id));
        }
    }
    earlier.sort_by(|left, right| left.0.cmp_precedence(&right.0));
    if earlier
        .windows(2)
        .any(|pair| pair[0].0.cmp_precedence(&pair[1].0) == Ordering::Equal)
    {
        return Err(invalid());
    }
    earlier
        .pop()
        .map(|(_, version, id)| Some(Baseline { version, id }))
        .ok_or_else(invalid)
}

pub(crate) fn verify_preflight<R: CrateRegistryRead>(
    source: &Path,
    receipt: &CrateReleaseAdmissionReceipt,
    registry: &R,
    runner_temp: &Path,
) -> Result<std::collections::BTreeMap<String, SemverEvidence>, OrchestratorError> {
    let executable = verify_checker(receipt, runner_temp)?;
    crate::verify_crate_release_source(source, &receipt.github.request.source_sha)?;
    let effective = crate::crate_release_controller::effective_release_intent(
        &receipt.intent,
        &receipt.github.request,
    );
    let workspace = crate::crate_release_workspace::crate_release_workspace_plan_with_registry(
        source,
        &receipt.github.request.source_sha,
        &effective,
        registry,
    )?;
    let mut evidence = std::collections::BTreeMap::new();
    for package in &receipt.packages {
        let mut blocked: Vec<String> = workspace
            .plan
            .graph
            .edges
            .iter()
            .filter(|edge| edge.from == package.source.name)
            .filter(|edge| {
                receipt.packages.iter().any(|other| {
                    other.source.name == edge.to
                        && matches!(
                            other.action,
                            crate::crate_release_controller::CrateReleaseAction::Publish
                        )
                })
            })
            .map(|edge| edge.to.clone())
            .collect();
        blocked.sort();
        blocked.dedup();
        if !blocked.is_empty() {
            eprintln!(
                "SemVer check deferred until selected prerequisite is published: {}",
                package.source.name
            );
            evidence.insert(
                package.source.name.clone(),
                SemverEvidence::Deferred {
                    predecessors: blocked,
                },
            );
            continue;
        }
        let item = if let Some(pending) = prior_no_baseline_pending(receipt, registry, package)? {
            pending
        } else {
            verify_one(source, receipt, registry, package, &executable)?
        };
        evidence.insert(package.source.name.clone(), item);
    }
    crate::verify_crate_release_source(source, &receipt.github.request.source_sha)?;
    Ok(evidence)
}

pub(crate) fn prior_no_baseline_pending<R: CrateRegistryRead>(
    receipt: &CrateReleaseAdmissionReceipt,
    registry: &R,
    package: &crate::crate_release_controller::CrateReleasePlannedPackage,
) -> Result<Option<SemverEvidence>, OrchestratorError> {
    use crate::crate_release_controller::CrateReleaseAction;
    let Some(creation) = &receipt.intent.new_crates else {
        return Ok(None);
    };
    if receipt.intent.journal_predecessor.is_none()
        || receipt.intent.authentication
            != velnor_actions_contract::config::ReleaseAuthentication::BootstrapToken
        || receipt.github.request.event != "workflow_dispatch"
        || creation.source_sha != receipt.github.request.source_sha
        || creation.packages.get(&package.source.name) != Some(&package.source.version)
    {
        return Ok(None);
    }
    let published = match &package.action {
        CrateReleaseAction::RepairTag { registry }
        | CrateReleaseAction::RepairRelease { registry }
        | CrateReleaseAction::Reconciled { registry, .. } => registry,
        CrateReleaseAction::Publish => return Ok(None),
    };
    let inventory =
        inspect_crate_versions(registry, &package.source.name).map_err(|_| invalid())?;
    if inventory.versions.len() != 1
        || inventory
            .versions
            .get(&package.source.version)
            .is_none_or(|v| v.yanked)
        || published.checksum != published.downloaded_checksum
    {
        return Err(invalid());
    }
    let checker =
        velnor_actions_mise::catalog::qualified_cargo_semver_checks(&receipt.intent.runner)
            .ok_or_else(invalid)?;
    Ok(Some(SemverEvidence::PriorNoBaselinePending {
        checker_sha256: checker.executable_sha256.into(),
        target_checksum: published.checksum.clone(),
    }))
}

pub(crate) fn verify_before_upload<R: CrateRegistryRead>(
    source: &Path,
    receipt: &CrateReleaseAdmissionReceipt,
    registry: &R,
    runner_temp: &Path,
    name: &str,
) -> Result<VerifiedSemver, OrchestratorError> {
    let executable = verify_checker(receipt, runner_temp)?;
    crate::verify_crate_release_source(source, &receipt.github.request.source_sha)?;
    let package = receipt
        .packages
        .iter()
        .find(|package| package.source.name == name)
        .ok_or_else(invalid)?;
    let evidence = verify_one(source, receipt, registry, package, &executable)?;
    crate::verify_crate_release_source(source, &receipt.github.request.source_sha)?;
    let intent =
        velnor_actions_contract::canonical_json_bytes(&receipt.intent).map_err(|_| invalid())?;
    Ok(VerifiedSemver {
        package: package.source.name.clone(),
        version: package.source.version.clone(),
        source_sha: receipt.github.request.source_sha.clone(),
        intent_sha256: crate::generator::sha256_hex(&intent),
        evidence,
    })
}

fn verify_checker(
    receipt: &CrateReleaseAdmissionReceipt,
    runner_temp: &Path,
) -> Result<std::path::PathBuf, OrchestratorError> {
    let qualified =
        velnor_actions_mise::catalog::qualified_cargo_semver_checks(&receipt.intent.runner)
            .ok_or_else(invalid)?;
    let relative = "velnor-semver/cargo-semver-checks";
    let binary = match crate::safe_read::read_repo_bytes(runner_temp, relative, 128 * 1024 * 1024)?
    {
        crate::safe_read::RepoBytes::Bytes(bytes) if !bytes.is_empty() => bytes,
        _ => return Err(invalid()),
    };
    if crate::generator::sha256_hex(&binary) != qualified.executable_sha256 {
        return Err(invalid());
    }
    Ok(runner_temp.join(relative))
}

fn verify_one<R: CrateRegistryRead>(
    source: &Path,
    receipt: &CrateReleaseAdmissionReceipt,
    registry: &R,
    package: &crate::crate_release_controller::CrateReleasePlannedPackage,
    executable: &Path,
) -> Result<SemverEvidence, OrchestratorError> {
    let checker_sha256 =
        velnor_actions_mise::catalog::qualified_cargo_semver_checks(&receipt.intent.runner)
            .ok_or_else(invalid)?
            .executable_sha256
            .to_owned();
    let Some(previous) = baseline(registry, &package.source.name, &package.source.version)? else {
        return Ok(SemverEvidence::NoBaseline { checker_sha256 });
    };
    let features = receipt
        .intent
        .publish_features
        .get(&package.source.name)
        .ok_or_else(invalid)?;
    let (archive, baseline_checksum) =
        download_baseline(registry, &package.source.name, &previous)?;
    run_check(
        source,
        &receipt.intent.rust_version,
        &package.source.manifest,
        &package.source.name,
        (&previous, &archive),
        features,
        executable,
    )?;
    Ok(SemverEvidence::Checked {
        baseline: previous.version,
        baseline_checksum,
        checker_sha256,
    })
}

fn download_baseline<R: CrateRegistryRead>(
    registry: &R,
    package: &str,
    baseline: &Baseline,
) -> Result<(Vec<u8>, String), OrchestratorError> {
    let version = baseline.version.replace('+', "%2B");
    let response = registry.get(&format!("/api/v1/crates/{package}/{version}"))?;
    if response.status != 200 || response.body.len() > 8 * 1024 * 1024 {
        return Err(invalid());
    }
    let reply: BaselineReply = serde_json::from_slice(&response.body).map_err(|_| invalid())?;
    let item = reply.version;
    let checksum = &item.checksum;
    if checksum.len() != 64
        || !checksum
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || item.id != baseline.id
        || item.package != package
        || item.num != baseline.version
        || item.yanked
    {
        return Err(invalid());
    }
    let archive = registry.get(&format!("/crates/{package}/{package}-{version}.crate"))?;
    if archive.status != 200
        || archive.body.len() > 64 * 1024 * 1024
        || crate::generator::sha256_hex(&archive.body) != *checksum
    {
        return Err(invalid());
    }
    Ok((archive.body, item.checksum))
}

fn extract_baseline(
    bytes: &[u8],
    package: &str,
    version: &str,
    destination: &Path,
) -> Result<(), OrchestratorError> {
    let files =
        crate::crate_release_archive::read_archive(bytes, &format!("{package}-{version}/"))?;
    let manifest = files.get("Cargo.toml").ok_or_else(invalid)?;
    let manifest: toml::Value =
        toml::from_str(std::str::from_utf8(&manifest.1).map_err(|_| invalid())?)
            .map_err(|_| invalid())?;
    if manifest["package"]["name"].as_str() != Some(package)
        || manifest["package"]["version"].as_str() != Some(version)
    {
        return Err(invalid());
    }
    for (path, (mode, bytes)) in files {
        let output = destination.join(path);
        std::fs::create_dir_all(output.parent().ok_or_else(invalid)?).map_err(|_| invalid())?;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)
            .map_err(|_| invalid())?;
        file.write_all(&bytes).map_err(|_| invalid())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            file.set_permissions(std::fs::Permissions::from_mode(0o600 | mode))
                .map_err(|_| invalid())?;
        }
        #[cfg(not(unix))]
        {
            let _ = mode;
            return Err(invalid());
        }
    }
    Ok(())
}

fn run_check(
    source: &Path,
    rust: &str,
    manifest: &str,
    package: &str,
    baseline: (&Baseline, &[u8]),
    features: &[String],
    executable: &Path,
) -> Result<(), OrchestratorError> {
    let temp = tempfile::tempdir().map_err(|_| invalid())?;
    let home = temp.path().join("cargo-home");
    std::fs::create_dir(&home).map_err(|_| invalid())?;
    let baseline_root = temp.path().join("baseline");
    std::fs::create_dir(&baseline_root).map_err(|_| invalid())?;
    extract_baseline(baseline.1, package, &baseline.0.version, &baseline_root)?;
    let cargo = source_cargo_path(rust, temp.path())?;
    let mut args = vec![
        OsString::from("-i"),
        format!("HOME={}", temp.path().display()).into(),
        format!("CARGO_HOME={}", home.display()).into(),
        format!("CARGO_TARGET_DIR={}", temp.path().join("target").display()).into(),
        format!("CARGO={}", cargo.display()).into(),
    ];
    args.extend(source_compiler_env(rust)?);
    args.extend([
        executable.as_os_str().to_owned(),
        "check-release".into(),
        "--manifest-path".into(),
        source.join(manifest).into_os_string(),
        "--package".into(),
        package.into(),
        "--baseline-root".into(),
        baseline_root.into_os_string(),
        "--default-features".into(),
    ]);
    if !features.is_empty() {
        args.extend(["--current-features".into(), features.join(",").into()]);
    }
    let command = PinnedToolExec::new(vec![PinnedTool::Rust], std::ffi::OsStr::new("env"), args)
        .map_err(|_| invalid())?
        .command(&source_tool_catalog(rust).map_err(|_| invalid())?)
        .map_err(|_| invalid())?
        .with_cwd(source.to_path_buf());
    let deadline = velnor_actions_mise::CheckDeadline::after(Duration::from_secs(600))
        .map_err(|_| invalid())?;
    let (output, _) = command
        .run_quiescent_until(1024 * 1024, deadline)
        .map_err(|_| invalid())?;
    if !output.success {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crate_release_registry::CrateRegistryResponse;

    struct Registry {
        status: u16,
        body: Vec<u8>,
    }
    impl CrateRegistryRead for Registry {
        fn get(&self, _path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
            Ok(CrateRegistryResponse {
                status: self.status,
                body: self.body.clone(),
            })
        }
    }

    #[test]
    fn baseline_requires_authoritative_registry_and_uses_latest_previous_version()
    -> Result<(), Box<dyn std::error::Error>> {
        let body = serde_json::json!({"crate":{"id":"demo","name":"demo","versions":[1,2,3]},"versions":[{"id":1,"crate":"demo","num":"0.1.0","yanked":false},{"id":2,"crate":"demo","num":"0.1.1","yanked":true},{"id":3,"crate":"demo","num":"0.2.0","yanked":false}]}).to_string().into_bytes();
        let registry = Registry { status: 200, body };
        assert_eq!(
            baseline(&registry, "demo", "0.1.2")?,
            Some(Baseline {
                version: "0.1.0".into(),
                id: 1
            })
        );
        assert!(baseline(&registry, "demo", "0.0.1").is_err());
        let yanked = serde_json::json!({"crate":{"id":"demo","name":"demo","versions":[1]},"versions":[{"id":1,"crate":"demo","num":"0.1.0","yanked":true}]}).to_string().into_bytes();
        assert!(
            baseline(
                &Registry {
                    status: 200,
                    body: yanked
                },
                "demo",
                "0.1.1"
            )
            .is_err()
        );
        let ambiguous = serde_json::json!({"crate":{"id":"demo","name":"demo","versions":[1,2]},"versions":[{"id":1,"crate":"demo","num":"0.1.0+one","yanked":false},{"id":2,"crate":"demo","num":"0.1.0+two","yanked":false}]}).to_string().into_bytes();
        assert!(
            baseline(
                &Registry {
                    status: 200,
                    body: ambiguous
                },
                "demo",
                "0.1.1"
            )
            .is_err()
        );
        assert_eq!(
            baseline(
                &Registry {
                    status: 404,
                    body: vec![]
                },
                "demo",
                "0.1.1"
            )?,
            None
        );
        for status in [403, 429, 500] {
            assert!(
                baseline(
                    &Registry {
                        status,
                        body: vec![]
                    },
                    "demo",
                    "0.1.1"
                )
                .is_err()
            );
        }
        assert!(
            baseline(
                &Registry {
                    status: 200,
                    body: b"{}".to_vec()
                },
                "demo",
                "0.1.1"
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn baseline_archive_is_bound_to_registry_identity_and_checksum()
    -> Result<(), Box<dyn std::error::Error>> {
        struct ArchiveRegistry {
            detail: Vec<u8>,
            archive: Vec<u8>,
        }
        impl CrateRegistryRead for ArchiveRegistry {
            fn get(&self, path: &str) -> Result<CrateRegistryResponse, OrchestratorError> {
                let body = if path == "/api/v1/crates/demo/0.1.0" {
                    self.detail.clone()
                } else if path == "/crates/demo/demo-0.1.0.crate" {
                    self.archive.clone()
                } else {
                    return Err(invalid());
                };
                Ok(CrateRegistryResponse { status: 200, body })
            }
        }
        let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        let mut tar = tar::Builder::new(encoder);
        let manifest = b"[package]\nname='demo'\nversion='0.1.0'\n";
        let mut header = tar::Header::new_gnu();
        header.set_size(manifest.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, "demo-0.1.0/Cargo.toml", manifest.as_slice())?;
        let archive = tar.into_inner()?.finish()?;
        let checksum = crate::generator::sha256_hex(&archive);
        let detail = serde_json::json!({"version":{"id":1,"crate":"demo","num":"0.1.0","yanked":false,"checksum":checksum}}).to_string().into_bytes();
        let mut registry = ArchiveRegistry { detail, archive };
        let previous = Baseline {
            version: "0.1.0".into(),
            id: 1,
        };
        let (bytes, observed) = download_baseline(&registry, "demo", &previous)?;
        assert_eq!(observed, checksum);
        let temp = tempfile::tempdir()?;
        extract_baseline(&bytes, "demo", "0.1.0", temp.path())?;
        assert_eq!(std::fs::read(temp.path().join("Cargo.toml"))?, manifest);
        let original = registry.detail.clone();
        for (field, value) in [
            ("id", serde_json::json!(2)),
            ("crate", serde_json::json!("other")),
            ("num", serde_json::json!("0.1.1")),
            ("yanked", serde_json::json!(true)),
            ("checksum", serde_json::json!("a".repeat(64))),
        ] {
            let mut detail: serde_json::Value = serde_json::from_slice(&original)?;
            detail["version"][field] = value;
            registry.detail = serde_json::to_vec(&detail)?;
            assert!(
                download_baseline(&registry, "demo", &previous).is_err(),
                "accepted {field}"
            );
        }
        registry.detail = format!(r#"{{"version":{{"id":1,"crate":"demo","num":"0.1.0","num":"0.1.1","yanked":false,"checksum":"{checksum}"}}}}"#).into_bytes();
        assert!(download_baseline(&registry, "demo", &previous).is_err());
        registry.detail = original;
        registry.archive.push(0);
        assert!(download_baseline(&registry, "demo", &previous).is_err());
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn fixture_verified_semver(
    receipt: &CrateReleaseAdmissionReceipt,
    name: &str,
) -> Result<VerifiedSemver, OrchestratorError> {
    let package = receipt
        .packages
        .iter()
        .find(|p| p.source.name == name)
        .ok_or_else(invalid)?;
    let intent =
        velnor_actions_contract::canonical_json_bytes(&receipt.intent).map_err(|_| invalid())?;
    Ok(VerifiedSemver {
        package: name.to_owned(),
        version: package.source.version.clone(),
        source_sha: receipt.github.request.source_sha.clone(),
        intent_sha256: crate::generator::sha256_hex(&intent),
        evidence: SemverEvidence::NoBaseline {
            checker_sha256: velnor_actions_mise::catalog::qualified_cargo_semver_checks(
                "ubuntu-24.04",
            )
            .ok_or_else(invalid)?
            .executable_sha256
            .into(),
        },
    })
}

#[cfg(test)]
mod evidence_tests {
    use super::{SemverEvidence, VerifiedSemver};

    fn proof() -> VerifiedSemver {
        VerifiedSemver {
            package: "demo".into(),
            version: "0.1.1".into(),
            source_sha: "a".repeat(40),
            intent_sha256: "b".repeat(64),
            evidence: SemverEvidence::Checked {
                baseline: "0.1.0".into(),
                baseline_checksum: "d".repeat(64),
                checker_sha256: "bc9eb06ce04e97d6160c875ffdee1f6e56793e423dea025f344826be03e66829"
                    .into(),
            },
        }
    }

    #[test]
    fn preflight_revalidation_accepts_only_verified_deferred_progress() {
        use std::collections::BTreeMap;
        let deferred = SemverEvidence::Deferred {
            predecessors: vec!["dep".into()],
        };
        let checked = proof().evidence;
        let map = |v| BTreeMap::from([("demo".to_owned(), v)]);
        assert!(super::matches_revalidated_preflight(
            &map(deferred.clone()),
            &map(checked.clone())
        ));
        assert!(super::matches_revalidated_preflight(
            &map(deferred.clone()),
            &map(deferred)
        ));
        assert!(!super::matches_revalidated_preflight(
            &map(checked.clone()),
            &BTreeMap::new()
        ));
        let mut changed_checksum = checked.clone();
        if let SemverEvidence::Checked {
            baseline_checksum, ..
        } = &mut changed_checksum
        {
            *baseline_checksum = "e".repeat(64);
        }
        assert!(!super::matches_revalidated_preflight(
            &map(checked.clone()),
            &map(changed_checksum)
        ));
        let mut changed = checked.clone();
        if let SemverEvidence::Checked { baseline, .. } = &mut changed {
            *baseline = "0.0.9".into();
        }
        assert!(!super::matches_revalidated_preflight(
            &map(checked),
            &map(changed)
        ));
    }

    #[test]
    fn final_proof_is_bound_to_package_source_and_intent() {
        assert!(
            proof()
                .into_bound_evidence("demo", "0.1.1", &"a".repeat(40), &"b".repeat(64))
                .is_ok()
        );
        assert!(
            proof()
                .into_bound_evidence("other", "0.1.1", &"a".repeat(40), &"b".repeat(64))
                .is_err()
        );
        assert!(
            proof()
                .into_bound_evidence("demo", "0.2.0", &"a".repeat(40), &"b".repeat(64))
                .is_err()
        );
        assert!(
            proof()
                .into_bound_evidence("demo", "0.1.1", &"f".repeat(40), &"b".repeat(64))
                .is_err()
        );
        assert!(
            proof()
                .into_bound_evidence("demo", "0.1.1", &"a".repeat(40), &"f".repeat(64))
                .is_err()
        );
        let mut deferred = proof();
        deferred.evidence = SemverEvidence::Deferred {
            predecessors: vec!["other".into()],
        };
        assert!(
            deferred
                .into_bound_evidence("demo", "0.1.1", &"a".repeat(40), &"b".repeat(64))
                .is_err()
        );
    }
}
