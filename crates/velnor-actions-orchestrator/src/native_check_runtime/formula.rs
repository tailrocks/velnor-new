use std::{collections::BTreeMap, path::Path};

use velnor_actions_contract::homebrew_spec::{
    HomebrewFormulaReleaseLiterals, parse_generated_formula, render_homebrew_formula,
    validate_homebrew_projection,
};
use velnor_actions_contract::package_release::PackageReleaseChannel;
use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};
use velnor_actions_contract::config::NativeHomebrewOperation;
use velnor_actions_contract::digest_b3;

pub(crate) fn verify_formula(
    root: &Path,
    channel: PackageReleaseChannel,
    identity: &crate::internal_plan::native_homebrew_checks::NativeCheckIdentityExtension,
    event: velnor_actions_contract::WorkflowEvent,
    deadline: CheckDeadline,
) -> Result<(Option<String>, Vec<u8>), OrchestratorError> {
    validate_formula_identity(identity, channel)?;
    if crate::internal_plan::native_homebrew_checks::caller_repository(root)?
        != identity.tap_repository
    {
        return Err(internal("native_check_repository_identity"));
    }
    let authority = crate::homebrew::authority::load_protected_homebrew_authority(
        root,
        &identity.tap_repository,
        event,
        crate::internal_plan::native_homebrew_checks::pull_request_number_from_env(event)?,
        &identity.head,
        deadline,
    )?;
    validate_formula_authority(&authority, identity)?;
    let bytes = read_authenticated_formula(root, identity, deadline)?;
    let seed_base =
        crate::homebrew::structural_cutover::verify_seed_output_base(root, &authority, deadline)?;
    if let Some(witness) = seed_base {
        crate::homebrew::structural_cutover::validate_structural_candidate(
            root, &authority, &witness, deadline,
        )?;
        cutover::verify_seed_cutover_formula_bytes(&authority, channel, &bytes, &witness, deadline)
    } else {
        verify_protected_formula_bytes(&authority, channel, &bytes, deadline)
    }
}

fn validate_formula_identity(
    identity: &crate::internal_plan::native_homebrew_checks::NativeCheckIdentityExtension,
    channel: PackageReleaseChannel,
) -> Result<(), OrchestratorError> {
    if !matches!(
        identity.operation,
        NativeHomebrewOperation::FormulaContract | NativeHomebrewOperation::Install
    ) || identity.formula_channel != Some(channel)
    {
        Err(internal("native_check_formula_identity"))
    } else {
        Ok(())
    }
}

fn validate_formula_authority(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    identity: &crate::internal_plan::native_homebrew_checks::NativeCheckIdentityExtension,
) -> Result<(), OrchestratorError> {
    if authority.tap_repository_id() != identity.tap_repository_id
        || authority.tap_base_sha() != identity.tap_base_sha
    {
        Err(internal("native_check_protected_authority_identity"))
    } else {
        Ok(())
    }
}

fn read_authenticated_formula(
    root: &Path,
    identity: &crate::internal_plan::native_homebrew_checks::NativeCheckIdentityExtension,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    let tree = crate::internal_plan::native_homebrew_checks::read_checked_tap_tree(
        root, identity, deadline,
    )?;
    let path = identity
        .formula_path
        .as_deref()
        .ok_or_else(|| internal("native_check_formula_path_missing"))?;
    if tree
        .iter()
        .find(|entry| entry.path == path)
        .map(|entry| entry.blob_sha.as_str())
        != identity.formula_blob_sha.as_deref()
    {
        return Err(internal("native_check_formula_blob_identity"));
    }
    match crate::safe_read::read_repo_bytes_until(root, path, 1024 * 1024, deadline)? {
        crate::safe_read::RepoBytes::Bytes(bytes) => Ok(bytes),
        crate::safe_read::RepoBytes::Absent => Err(internal("native_check_formula_missing")),
    }
}

/// Verify caller-supplied formula bytes against protected policy and the public release.
pub(crate) fn verify_protected_formula_bytes(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    channel: PackageReleaseChannel,
    bytes: &[u8],
    deadline: CheckDeadline,
) -> Result<(Option<String>, Vec<u8>), OrchestratorError> {
    let mut release_plan = authority.plan().clone();
    release_plan.channel = channel;
    let projection = validate_homebrew_projection(authority.spec(), &release_plan)
        .map_err(|_| internal("native_check_formula_projection"))?;
    let parsed = parse_generated_formula(authority.spec(), &projection, bytes)
        .map_err(|_| internal("native_check_formula_invalid"))?;
    if channel == PackageReleaseChannel::Stable && parsed.is_disabled_stable_sentinel() {
        let protected_base = read_protected_formula(authority, channel, deadline)?;
        verify_stable_sentinel_transition(authority.spec(), &projection, bytes, &protected_base)?;
        return Ok((None, bytes.to_vec()));
    }
    let source_sha = formula_source_sha(channel, authority, &parsed)?;
    let tag = formula_tag(channel, &parsed, &source_sha)?;
    if parsed.source_sha() != Some(source_sha.as_str()) || parsed.tag() != Some(tag.as_str()) {
        return Err(internal("native_check_formula_selector_mismatch"));
    }
    let (receipt_digest, release) = verify_formula_release(
        authority,
        release_plan,
        channel,
        &projection,
        (tag, source_sha),
        bytes,
        deadline,
    )?;
    let protected_base = read_protected_formula(authority, channel, deadline)?;
    validate_formula_upgrade_history(authority, channel, &protected_base, bytes, &release)?;
    Ok((Some(receipt_digest), bytes.to_vec()))
}

fn read_protected_formula(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    channel: PackageReleaseChannel,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    let formula = authority
        .spec()
        .channels
        .iter()
        .find(|formula| formula.channel == channel)
        .ok_or_else(|| internal("native_check_formula_channel_missing"))?;
    authority.protected_tap_file(&formula.destination, 1024 * 1024, deadline)
}

fn validate_formula_upgrade_history(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    channel: PackageReleaseChannel,
    protected_base: &[u8],
    candidate: &[u8],
    release: &velnor_actions_contract::homebrew_formula::VerifiedPackageRelease,
) -> Result<(), OrchestratorError> {
    let formula = authority
        .spec()
        .channels
        .iter()
        .find(|formula| formula.channel == channel)
        .ok_or_else(|| internal("native_check_formula_channel_missing"))?;
    let mut plan = authority.plan().clone();
    plan.channel = channel;
    validate_candidate_upgrade(
        crate::tap_prepare::FormulaCandidateInput {
            spec: Some(authority.spec()),
            source_repository: &authority.plan().repository,
            tap_repository: &authority.tap().repository,
            formula_path: &formula.destination,
            base_sha: authority.tap_base_sha(),
            base_formula: protected_base,
            proposed_formula: None,
            plan: &plan,
            release,
        },
        candidate,
    )
}

fn validate_candidate_upgrade(
    input: crate::tap_prepare::FormulaCandidateInput<'_>,
    candidate: &[u8],
) -> Result<(), OrchestratorError> {
    let canonical = crate::tap_prepare::derive_formula_candidate(input)
        .map_err(|_| internal("native_check_formula_upgrade_history"))?;
    if canonical != candidate {
        return Err(internal("native_check_formula_upgrade_history"));
    }
    Ok(())
}

fn verify_stable_sentinel_transition(
    spec: &velnor_actions_contract::homebrew_spec::HomebrewFormulaSpec,
    projection: &velnor_actions_contract::homebrew_spec::ValidatedHomebrewProjection,
    candidate: &[u8],
    protected_base: &[u8],
) -> Result<(), OrchestratorError> {
    verify_stable_sentinel(spec, projection, candidate)?;
    verify_stable_sentinel(spec, projection, protected_base)
}

pub(super) fn verify_stable_sentinel(
    spec: &velnor_actions_contract::homebrew_spec::HomebrewFormulaSpec,
    projection: &velnor_actions_contract::homebrew_spec::ValidatedHomebrewProjection,
    bytes: &[u8],
) -> Result<(), OrchestratorError> {
    let sentinel = HomebrewFormulaReleaseLiterals::stable_sentinel(spec, projection)
        .map_err(|_| internal("native_check_stable_sentinel"))?;
    if render_homebrew_formula(spec, &sentinel)
        .map_err(|_| internal("native_check_formula_render"))?
        != bytes
    {
        return Err(internal("native_check_formula_sentinel_mismatch"));
    }
    Ok(())
}

fn formula_source_sha(
    channel: PackageReleaseChannel,
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    parsed: &velnor_actions_contract::homebrew_spec::HomebrewFormulaReleaseLiterals,
) -> Result<String, OrchestratorError> {
    match channel {
        PackageReleaseChannel::Preview => Ok(authority.authority_sha().to_owned()),
        PackageReleaseChannel::Stable => parsed
            .source_sha()
            .map(str::to_owned)
            .ok_or_else(|| internal("native_check_stable_source_sha_missing")),
    }
}

fn formula_tag(
    channel: PackageReleaseChannel,
    parsed: &velnor_actions_contract::homebrew_spec::HomebrewFormulaReleaseLiterals,
    source_sha: &str,
) -> Result<String, OrchestratorError> {
    match channel {
        PackageReleaseChannel::Preview => Ok(format!("preview-{source_sha}")),
        PackageReleaseChannel::Stable => parsed
            .tag()
            .map(str::to_owned)
            .ok_or_else(|| internal("native_check_stable_tag_missing")),
    }
}

fn verify_formula_release(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    release_plan: velnor_actions_contract::package_release_plan::PackageReleasePlan,
    channel: PackageReleaseChannel,
    projection: &velnor_actions_contract::homebrew_spec::ValidatedHomebrewProjection,
    selector: (String, String),
    bytes: &[u8],
    deadline: CheckDeadline,
) -> Result<
    (
        String,
        velnor_actions_contract::homebrew_formula::VerifiedPackageRelease,
    ),
    OrchestratorError,
> {
    let (tag, source_sha) = selector;
    let policy =
        crate::package_release_public::PackageReleasePublicPolicy::from_trusted_source_authority(
            release_plan,
            authority.source_repository_id(),
            authority.authority_sha().to_owned(),
        )?;
    let selection = crate::package_release_public::PackageReleasePublicSelection::new(
        channel, tag, source_sha,
    )?;
    let release = crate::package_release_public::verify_public_package_release(
        &policy, &selection, deadline,
    )?;
    let literals = formula_release_literals(authority, projection, &release)?;
    if render_homebrew_formula(authority.spec(), &literals)
        .map_err(|_| internal("native_check_formula_render"))?
        != bytes
    {
        return Err(internal("native_check_formula_release_mismatch"));
    }
    let receipt = release.receipt().clone();
    let receipt_digest = digest_b3(
        &receipt
            .canonical_bytes()
            .map_err(|_| internal("native_check_release_receipt_encode"))?,
    );
    let verified = velnor_actions_contract::homebrew_formula::VerifiedPackageRelease {
        receipt_digest: receipt_digest.clone(),
        receipt,
    };
    Ok((receipt_digest, verified))
}

pub(super) fn formula_release_literals(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    projection: &velnor_actions_contract::homebrew_spec::ValidatedHomebrewProjection,
    release: &crate::package_release_public::VerifiedPublicPackageRelease,
) -> Result<HomebrewFormulaReleaseLiterals, OrchestratorError> {
    let mut asset_hashes = BTreeMap::new();
    for selected in projection.selected_payloads() {
        let asset = release
            .receipt()
            .assets
            .iter()
            .find(|asset| asset.name == selected.asset_name())
            .ok_or_else(|| internal("native_check_formula_asset_missing"))?;
        asset_hashes.insert(asset.name.clone(), asset.sha256.clone());
    }
    HomebrewFormulaReleaseLiterals::new(
        authority.spec(),
        projection,
        release.receipt().source.commit_sha.clone(),
        release.receipt().version.value.clone(),
        release.receipt().tag.clone(),
        asset_hashes,
    )
    .map_err(|_| internal("native_check_formula_literals"))
}

#[path = "formula_cutover.rs"]
mod cutover;

pub(crate) use cutover::render_current_preview_formula;

#[cfg(test)]
#[path = "formula_tests.rs"]
mod tests;
