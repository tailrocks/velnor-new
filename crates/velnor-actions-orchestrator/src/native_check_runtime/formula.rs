use std::collections::BTreeMap;
use std::path::Path;

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
    verify_protected_formula_bytes(&authority, channel, &bytes, deadline)
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
        verify_stable_sentinel(authority, &projection, bytes)?;
        return Ok((None, bytes.to_vec()));
    }
    let source_sha = formula_source_sha(channel, authority, &parsed)?;
    let tag = formula_tag(channel, &parsed, &source_sha)?;
    if parsed.source_sha() != Some(source_sha.as_str()) || parsed.tag() != Some(tag.as_str()) {
        return Err(internal("native_check_formula_selector_mismatch"));
    }
    verify_formula_release(
        authority,
        release_plan,
        channel,
        &projection,
        (tag, source_sha),
        bytes,
        deadline,
    )
    .map(|digest| (Some(digest), bytes.to_vec()))
}

fn verify_stable_sentinel(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    projection: &velnor_actions_contract::homebrew_spec::ValidatedHomebrewProjection,
    bytes: &[u8],
) -> Result<(), OrchestratorError> {
    let sentinel = HomebrewFormulaReleaseLiterals::stable_sentinel(authority.spec(), projection)
        .map_err(|_| internal("native_check_stable_sentinel"))?;
    if render_homebrew_formula(authority.spec(), &sentinel)
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
) -> Result<String, OrchestratorError> {
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
    Ok(digest_b3(&release.receipt().canonical_bytes().map_err(
        |_| internal("native_check_release_receipt_encode"),
    )?))
}

fn formula_release_literals(
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
