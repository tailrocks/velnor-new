//! Canonical preparation proof construction and persistence.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use velnor_actions_contract::{
    PackageReleasePlan, VerifiedPackageRelease, canonical_json_bytes,
    config::HomebrewTapConfig,
    homebrew_formula::{HomebrewFormulaCandidate, validate_homebrew_formula_candidate},
    homebrew_spec::{
        HomebrewFormulaReleaseLiterals, HomebrewFormulaSpec, parse_generated_formula,
        render_homebrew_formula, validate_homebrew_projection,
    },
};

use super::{
    HomebrewTapPreparationProof, OrchestratorError, TapPreparationDisposition, internal, sha256,
};

const CANDIDATE_DIRECTORY: &str = "tap-candidate";
static STAGING_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn is_full_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, Copy)]
pub(super) struct CandidateIdentity<'a> {
    pub(super) source_repository: &'a str,
    pub(super) tap: &'a HomebrewTapConfig,
    pub(super) release: &'a VerifiedPackageRelease,
    pub(super) spec: Option<&'a HomebrewFormulaSpec>,
    pub(super) sdk_artifact_digest: &'a str,
    pub(super) base_sha: &'a str,
    pub(super) old_sha: &'a str,
    pub(super) formula: &'a [u8],
    pub(super) plan: &'a PackageReleasePlan,
    pub(super) formula_path: &'a str,
    pub(super) disposition: TapPreparationDisposition,
}

/// Authenticated inputs required to render one canonical formula candidate.
#[derive(Clone, Copy)]
pub(crate) struct FormulaCandidateInput<'a> {
    pub(crate) spec: Option<&'a HomebrewFormulaSpec>,
    pub(crate) source_repository: &'a str,
    pub(crate) tap_repository: &'a str,
    pub(crate) formula_path: &'a str,
    pub(crate) base_sha: &'a str,
    pub(crate) base_formula: &'a [u8],
    /// Candidate bytes are supplied only for the source-qualified custom route.
    pub(crate) proposed_formula: Option<&'a [u8]>,
    pub(crate) plan: &'a PackageReleasePlan,
    pub(crate) release: &'a VerifiedPackageRelease,
}

/// Authenticated inputs needed to reconstruct a merged historical proof.
#[derive(Clone, Copy)]
pub(crate) struct HistoricalCandidateInput<'a> {
    pub(crate) source_repository: &'a str,
    pub(crate) tap: &'a HomebrewTapConfig,
    pub(crate) plan: &'a PackageReleasePlan,
    pub(crate) release: &'a VerifiedPackageRelease,
    pub(crate) spec: Option<&'a HomebrewFormulaSpec>,
    pub(crate) sdk_artifact_digest: &'a str,
    pub(crate) base_sha: &'a str,
    pub(crate) base_formula: &'a [u8],
    pub(crate) proposed_formula: &'a [u8],
}

pub(super) fn persist_candidate(
    temp: &Path,
    identity: CandidateIdentity<'_>,
) -> Result<HomebrewTapPreparationProof, OrchestratorError> {
    let directory = temp.join(CANDIDATE_DIRECTORY);
    let staging = staging_path(temp);
    if directory.exists() || fs::symlink_metadata(&directory).is_ok() {
        return Err(internal("tap_candidate_directory_occupied"));
    }
    for entry in fs::read_dir(temp).map_err(|_| internal("tap_candidate_staging_scan"))? {
        let entry = entry.map_err(|_| internal("tap_candidate_staging_scan"))?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(".tap-candidate-"))
        {
            return Err(internal("tap_candidate_staging_occupied"));
        }
    }
    fs::create_dir(&staging).map_err(|_| internal("tap_candidate_staging_create"))?;
    let result = (|| {
        crate::exclusive_write::write_exclusive(
            &staging.join("formula.rb"),
            identity.formula,
            "tap_candidate_formula",
        )?;
        let proof = candidate_proof(&identity)?;
        let bytes =
            canonical_json_bytes(&proof).map_err(|_| internal("tap_candidate_proof_encode"))?;
        crate::exclusive_write::write_exclusive(
            &staging.join("candidate.json"),
            &bytes,
            "tap_candidate_proof",
        )?;
        if fs::symlink_metadata(&directory).is_ok() {
            return Err(internal("tap_candidate_directory_occupied"));
        }
        fs::rename(&staging, &directory).map_err(|_| internal("tap_candidate_publish"))?;
        Ok(proof)
    })();
    if result.is_err()
        && fs::symlink_metadata(&staging)
            .is_ok_and(|meta| meta.is_dir() && !meta.file_type().is_symlink())
        && let Err(error) = fs::remove_dir_all(&staging)
    {
        eprintln!(
            "velnor_tap_candidate_stage_cleanup_failed:{:?}",
            error.kind()
        );
    }
    result
}

fn staging_path(temp: &Path) -> PathBuf {
    let sequence = STAGING_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    temp.join(format!(".tap-candidate-{}-{sequence}", std::process::id()))
}

/// Derive the sole canonical formula from the typed spec and authenticated receipt.
pub(crate) fn derive_native_formula_candidate(
    input: FormulaCandidateInput<'_>,
) -> Result<Vec<u8>, OrchestratorError> {
    let spec = input
        .spec
        .ok_or_else(|| internal("tap_native_formula_spec_missing"))?;
    let FormulaCandidateInput {
        source_repository,
        tap_repository,
        formula_path,
        base_sha,
        base_formula,
        proposed_formula: _,
        plan,
        release,
        spec: _,
    } = input;
    release
        .validate(source_repository, plan)
        .map_err(|_| internal("tap_formula_receipt_invalid"))?;
    let channel = spec
        .channels
        .iter()
        .find(|channel| channel.channel == plan.channel)
        .ok_or_else(|| internal("tap_formula_channel_missing"))?;
    if channel.destination != formula_path
        || channel.formula
            != Path::new(formula_path)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default()
        || !is_full_sha(base_sha)
        || !velnor_actions_contract::vcs::is_safe_github_repository(tap_repository)
    {
        return Err(internal("tap_formula_spec_destination_mismatch"));
    }
    let projection = validate_homebrew_projection(spec, plan)
        .map_err(|_| internal("tap_formula_plan_projection_invalid"))?;
    // Recognize the complete base template before release equality can yield a no-op.
    parse_generated_formula(spec, &projection, base_formula)
        .map_err(|_| internal("tap_formula_base_template_invalid"))?;
    let asset_digests = projection
        .selected_payloads()
        .iter()
        .map(|selected| {
            let asset = release
                .receipt
                .assets
                .iter()
                .find(|asset| asset.kind == velnor_actions_contract::package_release::PackageReleaseAssetKind::Payload && asset.name == selected.asset_name())
                .ok_or_else(|| internal("tap_formula_payload_missing"))?;
            Ok((asset.name.clone(), asset.sha256.clone()))
        })
        .collect::<Result<BTreeMap<_, _>, OrchestratorError>>()?;
    let literals = HomebrewFormulaReleaseLiterals::new(
        spec,
        &projection,
        release.receipt.source.commit_sha.clone(),
        release.receipt.version.value.clone(),
        release.receipt.tag.clone(),
        asset_digests,
    )
    .map_err(|_| internal("tap_formula_release_literals_invalid"))?;
    let rendered = render_homebrew_formula(spec, &literals)
        .map_err(|_| internal("tap_formula_render_failed"))?;
    parse_generated_formula(spec, &projection, &rendered)
        .map_err(|_| internal("tap_formula_render_not_canonical"))?;
    let independently_validated = validate_homebrew_formula_candidate(&HomebrewFormulaCandidate {
        source_repository,
        plan,
        tap_repository,
        formula_path,
        captured_base_sha: base_sha,
        base_bytes: base_formula,
        proposed_bytes: &rendered,
        release,
    })
    .map_err(|_| internal("tap_formula_literal_or_monotonicity_rejected"))?;
    if independently_validated != rendered {
        return Err(internal("tap_formula_literal_validation_not_canonical"));
    }
    Ok(independently_validated)
}

pub(crate) fn derive_formula_candidate(
    input: FormulaCandidateInput<'_>,
) -> Result<Vec<u8>, OrchestratorError> {
    if input.spec.is_some() {
        return derive_native_formula_candidate(input);
    }
    let proposed_formula = input
        .proposed_formula
        .ok_or_else(|| internal("tap_custom_formula_candidate_missing"))?;
    validate_homebrew_formula_candidate(&HomebrewFormulaCandidate {
        source_repository: input.source_repository,
        plan: input.plan,
        tap_repository: input.tap_repository,
        formula_path: input.formula_path,
        captured_base_sha: input.base_sha,
        base_bytes: input.base_formula,
        proposed_bytes: proposed_formula,
        release: input.release,
    })
    .map_err(|_| internal("tap_formula_literal_or_monotonicity_rejected"))
}

/// Reconstruct the original changed proof from trusted release and formula inputs.
/// This supports historical PR authentication when fresh tap main is already current.
pub(crate) fn reconstruct_historical_candidate_proof(
    input: HistoricalCandidateInput<'_>,
) -> Result<HomebrewTapPreparationProof, OrchestratorError> {
    let HistoricalCandidateInput {
        source_repository,
        tap,
        plan,
        release,
        spec,
        sdk_artifact_digest,
        base_sha,
        base_formula,
        proposed_formula,
    } = input;
    let formula_path = super::formula_path(tap, plan.channel)?;
    let canonical = derive_formula_candidate(FormulaCandidateInput {
        spec,
        source_repository,
        tap_repository: &tap.repository,
        formula_path,
        base_sha,
        base_formula,
        proposed_formula: Some(proposed_formula),
        plan,
        release,
    })
    .map_err(|_| internal("tap_historical_formula_rejected"))?;
    if canonical != proposed_formula {
        return Err(internal("tap_historical_formula_rejected"));
    }
    candidate_proof(&CandidateIdentity {
        source_repository,
        tap,
        release,
        spec,
        sdk_artifact_digest,
        base_sha,
        old_sha: &sha256(base_formula),
        formula: &canonical,
        plan,
        formula_path,
        disposition: TapPreparationDisposition::Changed,
    })
}

fn candidate_proof(
    candidate: &CandidateIdentity<'_>,
) -> Result<HomebrewTapPreparationProof, OrchestratorError> {
    let CandidateIdentity {
        source_repository,
        tap,
        release,
        spec,
        sdk_artifact_digest,
        base_sha,
        old_sha,
        formula,
        plan,
        formula_path,
        disposition,
    } = candidate;
    if !is_sha256(sdk_artifact_digest) {
        return Err(internal("tap_sdk_artifact_digest_invalid"));
    }
    Ok(HomebrewTapPreparationProof {
        schema: "velnor.homebrew-candidate.v4".to_owned(),
        channel: release.receipt.channel,
        disposition: *disposition,
        base_sha: (*base_sha).to_owned(),
        formula_path: (*formula_path).to_owned(),
        old_formula_sha256: (*old_sha).to_owned(),
        new_formula_sha256: sha256(formula),
        receipt_digest: release.receipt_digest.clone(),
        plan_digest: sha256(&canonical_json_bytes(plan).map_err(|_| internal("tap_plan_encode"))?),
        formula_spec_digest: spec
            .map(|spec| {
                spec.digest()
                    .map_err(|_| internal("tap_formula_spec_digest"))
            })
            .transpose()?,
        sdk_artifact_digest: (*sdk_artifact_digest).to_owned(),
        tap_config_digest: sha256(
            &canonical_json_bytes(tap).map_err(|_| internal("tap_policy_encode"))?,
        ),
        tap_repository: tap.repository.clone(),
        source_repository: (*source_repository).to_owned(),
        source_sha: release.receipt.source.commit_sha.clone(),
        version: release.receipt.version.value.clone(),
        candidate_formula: "formula.rb".to_owned(),
    })
}

#[cfg(test)]
#[path = "tap_prepare_candidate_tests.rs"]
mod tests;
