// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, fs, path::Path};

use serde::{Deserialize, Serialize};
use velnor_actions_contract::{
    PackageReleasePlan, canonical_json_bytes,
    homebrew_spec::{HomebrewFormulaSpec, validate_homebrew_projection},
    package_release::{PackageReleaseReceipt, parse_package_release_receipt},
};

use super::{HistoricalRecord, absolute_deadline, load_catalog, sha256, verify_historical_pair};
use crate::{
    OrchestratorError, homebrew::hosted_profile::VerifiedReleaseQualificationCandidate,
    internal::internal, release::package_release_public::VerifiedPublicPackageRelease,
};

#[path = "homebrew_qualification_handoff_fs.rs"]
mod handoff_fs;
use handoff_fs::{io, read_artifact, safe_file, write_new};

#[path = "homebrew_qualification_handoff_identity.rs"]
mod identity;
pub(super) use identity::require_credential_free_environment;
use identity::{catalog, context, require_unchanged_admission, validate_result};

#[path = "homebrew_qualification_handoff_zip.rs"]
mod bundle;

const ADMISSION_DIR: &str = "homebrew-admission";
const RESULT_DIR: &str = "homebrew-result";
const JSON_LIMIT: u64 = 1024 * 1024;
const ASSET_LIMIT: u64 = 256 * 1024 * 1024;
const FILE_LIMIT: usize =
    2 * (velnor_actions_contract::package_release::MAX_PACKAGE_RELEASE_PRODUCTS + 1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    repository: String,
    repository_id: u64,
    source_sha: String,
    run_id: u64,
    run_attempt: u32,
    workflow_ref: String,
    workflow_sha: String,
    manifest_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileIdentity {
    size: u64,
    sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    plan: String,
    formula: String,
    a_record: String,
    d_record: String,
    recovery: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Admission {
    schema: String,
    context: Context,
    catalog: Catalog,
    authority_sha: String,
    files: BTreeMap<String, FileIdentity>,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QualificationResult {
    schema: String,
    context: Context,
    admission_sha256: String,
    transition: String,
    stable_sentinel: String,
}

pub(super) struct AdmittedQualification {
    admission: Admission,
    digest: String,
    a: AdmittedLocalRelease,
    d: AdmittedLocalRelease,
}

impl AdmittedQualification {
    pub(super) fn a(&self) -> &AdmittedLocalRelease {
        &self.a
    }
    pub(super) fn d(&self) -> &AdmittedLocalRelease {
        &self.d
    }
}

pub(super) struct AdmittedLocalRelease {
    receipt: PackageReleaseReceipt,
    assets: BTreeMap<String, Vec<u8>>,
}

impl AdmittedLocalRelease {
    pub(super) fn receipt(&self) -> &PackageReleaseReceipt {
        &self.receipt
    }

    pub(super) fn asset_bytes(&self, name: &str) -> Result<Vec<u8>, OrchestratorError> {
        self.assets
            .get(name)
            .cloned()
            .ok_or_else(|| internal("homebrew_handoff_asset_not_admitted"))
    }
}

pub fn admit_homebrew_qualification_from_env() -> Result<(), OrchestratorError> {
    require_linux()?;
    let deadline = absolute_deadline()?;
    let candidate = crate::homebrew::hosted_profile::verify_release_qualification_candidate()?;
    let (plan, spec, a, d) = load_catalog()?;
    let (verified_a, verified_d) = verify_historical_pair(&plan, &a, &d, deadline)?;
    let context = context(&candidate)?;
    let (admission, files) = export_admission(context, &plan, &spec, &verified_a, &verified_d)?;
    let root = Path::new(ADMISSION_DIR);
    fs::create_dir(root).map_err(|e| io(root, &e))?;
    let admission_bytes = encode(&admission)?;
    write_new(
        &root.join("admission.zip"),
        &bundle::encode_bundle(&admission_bytes, files)?,
    )?;
    write_new(&root.join("admission.json"), &admission_bytes)
}

pub fn finalize_homebrew_qualification_from_env() -> Result<(), OrchestratorError> {
    require_linux()?;
    let deadline = absolute_deadline()?;
    let candidate = crate::homebrew::hosted_profile::verify_release_qualification_candidate()?;
    let (plan, spec, a, d) = load_catalog()?;
    let admitted = read_admission(&candidate, &plan, &spec, &a, &d)?;
    let result_bytes = read_artifact(
        Path::new("homebrew-result-zip"),
        &env("VELNOR_HOME_RESULT_ARCHIVE_SHA256")?,
        number("VELNOR_HOME_RESULT_SIZE")?,
        JSON_LIMIT,
    )?;
    let expected_result_digest = env("VELNOR_HOME_RESULT_SHA256")?;
    validate_result(
        &result_bytes,
        &expected_result_digest,
        &admitted.admission.context,
        &admitted.digest,
    )?;
    let (verified_a, verified_d) = verify_historical_pair(&plan, &a, &d, deadline)?;
    let (fresh, _) =
        export_admission(context(&candidate)?, &plan, &spec, &verified_a, &verified_d)?;
    require_unchanged_admission(&admitted.admission, &fresh)
}

pub(super) fn read_admission(
    candidate: &VerifiedReleaseQualificationCandidate,
    plan: &PackageReleasePlan,
    spec: &HomebrewFormulaSpec,
    a: &HistoricalRecord,
    d: &HistoricalRecord,
) -> Result<AdmittedQualification, OrchestratorError> {
    let digest = env("VELNOR_HOME_ADMISSION_SHA256")?;
    let raw = read_artifact(
        Path::new("homebrew-admission-zip"),
        &env("VELNOR_HOME_ADMISSION_ARCHIVE_SHA256")?,
        number("VELNOR_HOME_ADMISSION_ARCHIVE_SIZE")?,
        bundle::RAW_LIMIT,
    )?;
    let mut archive = bundle::open_bundle(&raw)?;
    let bytes = bundle::read_entry(&mut archive, "admission.json", JSON_LIMIT)?;
    let admission = validate_admission(&bytes, &digest, &context(candidate)?, &catalog())?;
    let files = bundle::read_admitted_files(&mut archive, &admission)?;
    let a = read_release(&files, "A", &admission, plan, spec, a)?;
    let d = read_release(&files, "D", &admission, plan, spec, d)?;
    Ok(AdmittedQualification {
        admission,
        digest,
        a,
        d,
    })
}

fn validate_admission(
    bytes: &[u8],
    digest: &str,
    context: &Context,
    catalog: &Catalog,
) -> Result<Admission, OrchestratorError> {
    if bytes.len() as u64 > JSON_LIMIT || !is_sha256(digest) || sha256(bytes) != digest {
        return Err(internal("homebrew_handoff_admission_digest"));
    }
    let admission: Admission = parse_canonical(bytes)?;
    if admission.schema != "velnor.homebrew-qualification-admission.v1"
        || &admission.context != context
        || &admission.catalog != catalog
        || !is_sha1(&admission.authority_sha)
        || admission.files.len() > FILE_LIMIT
        || admission.files.iter().any(|(name, file)| {
            !safe_file(name)
                || file.size == 0
                || file.size > ASSET_LIMIT
                || !is_sha256(&file.sha256)
        })
    {
        return Err(internal("homebrew_handoff_admission_identity"));
    }
    Ok(admission)
}

fn read_release(
    files: &BTreeMap<String, Vec<u8>>,
    label: &str,
    admission: &Admission,
    plan: &PackageReleasePlan,
    spec: &HomebrewFormulaSpec,
    record: &HistoricalRecord,
) -> Result<AdmittedLocalRelease, OrchestratorError> {
    let receipt_name = format!("{label}/release.json");
    let bytes = bundle::admitted_bytes(files, &receipt_name, admission, JSON_LIMIT)?;
    if bytes.len() as u64 != record.release.receipt_size
        || sha256(&bytes) != record.release.receipt_sha256
    {
        return Err(internal("homebrew_handoff_receipt_pin"));
    }
    let receipt =
        parse_package_release_receipt(&bytes).map_err(|_| internal("homebrew_handoff_receipt"))?;
    receipt
        .validate_against_plan(plan)
        .map_err(|_| internal("homebrew_handoff_receipt_plan"))?;
    super::check_receipt_identity(&receipt, record)?;
    let mut assets = BTreeMap::new();
    for name in selected_assets(spec, plan)? {
        let identity = receipt
            .assets
            .iter()
            .find(|asset| asset.name == name)
            .ok_or_else(|| internal("homebrew_handoff_asset_missing"))?;
        let bytes =
            bundle::admitted_bytes(files, &format!("{label}/{name}"), admission, ASSET_LIMIT)?;
        if bytes.len() as u64 != identity.size || sha256(&bytes) != identity.sha256 {
            return Err(internal("homebrew_handoff_payload_identity"));
        }
        assets.insert(name, bytes);
    }
    if admission
        .files
        .keys()
        .filter(|name| name.starts_with(&format!("{label}/")))
        .count()
        != assets.len() + 1
    {
        return Err(internal("homebrew_handoff_asset_inventory"));
    }
    Ok(AdmittedLocalRelease { receipt, assets })
}

fn export_admission(
    context: Context,
    plan: &PackageReleasePlan,
    spec: &HomebrewFormulaSpec,
    a: &VerifiedPublicPackageRelease,
    d: &VerifiedPublicPackageRelease,
) -> Result<(Admission, BTreeMap<String, Vec<u8>>), OrchestratorError> {
    let mut files = BTreeMap::new();
    for (label, release) in [("A", a), ("D", d)] {
        let names = std::iter::once("release.json".to_owned()).chain(selected_assets(spec, plan)?);
        for name in names {
            files.insert(format!("{label}/{name}"), release.asset_bytes(&name)?);
        }
    }
    let identities = files
        .iter()
        .map(|(name, bytes)| {
            (
                name.clone(),
                FileIdentity {
                    size: bytes.len() as u64,
                    sha256: sha256(bytes),
                },
            )
        })
        .collect();
    let admission = Admission {
        schema: "velnor.homebrew-qualification-admission.v1".to_owned(),
        context,
        catalog: catalog(),
        authority_sha: a.authority_sha().to_owned(),
        files: identities,
    };
    let bytes = encode(&admission)?;
    validate_admission(
        &bytes,
        &sha256(&bytes),
        &admission.context,
        &admission.catalog,
    )?;
    Ok((admission, files))
}

fn selected_assets(
    spec: &HomebrewFormulaSpec,
    plan: &PackageReleasePlan,
) -> Result<Vec<String>, OrchestratorError> {
    let projection = validate_homebrew_projection(spec, plan)
        .map_err(|_| internal("homebrew_handoff_projection"))?;
    let expected = crate::internal_plan::native_homebrew_checks::expected_install_files(
        spec,
        &projection,
        velnor_actions_contract::config::CheckPlatform::MacosArm64,
    )?;
    let names = expected
        .into_iter()
        .map(|item| item.asset)
        .collect::<std::collections::BTreeSet<_>>();
    Ok(names.into_iter().collect())
}

pub(super) fn write_result(admitted: &AdmittedQualification) -> Result<(), OrchestratorError> {
    let root = Path::new(RESULT_DIR);
    fs::create_dir(root).map_err(|e| io(root, &e))?;
    write_new(&root.join("result.json"), &encode(&result_for(admitted))?)
}

fn result_for(admitted: &AdmittedQualification) -> QualificationResult {
    QualificationResult {
        schema: "velnor.homebrew-qualification-result.v1".to_owned(),
        context: admitted.admission.context.clone(),
        admission_sha256: admitted.digest.clone(),
        transition: "A-install-D-upgrade-preserved-kegs".to_owned(),
        stable_sentinel: "disabled".to_owned(),
    }
}

fn require_linux() -> Result<(), OrchestratorError> {
    if std::env::consts::OS != "linux" {
        return Err(internal("homebrew_handoff_linux_required"));
    }
    Ok(())
}

fn parse_canonical<T: serde::de::DeserializeOwned + Serialize>(
    bytes: &[u8],
) -> Result<T, OrchestratorError> {
    let text = std::str::from_utf8(bytes).map_err(|_| internal("homebrew_handoff_json"))?;
    let strict = velnor_actions_contract::parse_strict_json(text)
        .map_err(|_| internal("homebrew_handoff_json"))?;
    let parsed: T =
        serde_json::from_value(strict).map_err(|_| internal("homebrew_handoff_json"))?;
    if encode(&parsed)? != bytes {
        return Err(internal("homebrew_handoff_noncanonical"));
    }
    Ok(parsed)
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, OrchestratorError> {
    canonical_json_bytes(value).map_err(|_| internal("homebrew_handoff_encode"))
}
fn env(key: &str) -> Result<String, OrchestratorError> {
    std::env::var(key).map_err(|_| internal("homebrew_handoff_environment"))
}
fn number(key: &str) -> Result<u64, OrchestratorError> {
    let value = env(key)?;
    let parsed = value
        .parse::<u64>()
        .map_err(|_| internal("homebrew_handoff_number"))?;
    if parsed == 0 || parsed.to_string() != value {
        return Err(internal("homebrew_handoff_number"));
    }
    Ok(parsed)
}
fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
fn is_sha1(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[path = "homebrew_qualification_handoff/tests.rs"]
mod tests;
