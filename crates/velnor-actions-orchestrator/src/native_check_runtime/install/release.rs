use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_contract::homebrew_spec::{
    HomebrewFormulaReleaseLiterals, parse_generated_formula, render_homebrew_formula,
    validate_homebrew_projection,
};
use velnor_actions_contract::{
    config::NativeHomebrewOperation, package_release::PackageReleaseChannel,
};
use velnor_actions_mise::CheckDeadline;

use crate::OrchestratorError;
use velnor_actions_contract::digest_b3;

struct AuthorityFormula {
    authority: crate::homebrew::authority::VerifiedHomebrewAuthority,
    path: String,
    bytes: Vec<u8>,
}

pub(super) struct InstallSource {
    pub(super) authority: crate::homebrew::authority::VerifiedHomebrewAuthority,
    pub(super) formula_path: String,
    pub(super) formula_bytes: Vec<u8>,
    pub(super) projection: velnor_actions_contract::homebrew_spec::ValidatedHomebrewProjection,
    pub(super) literals: HomebrewFormulaReleaseLiterals,
    pub(super) release: crate::package_release_public::VerifiedPublicPackageRelease,
    pub(super) receipt_digest: String,
}

pub(super) fn load_install_source(
    root: &Path,
    channel: PackageReleaseChannel,
    identity: &crate::internal_plan::native_homebrew_checks::NativeCheckIdentityExtension,
    event: velnor_actions_contract::WorkflowEvent,
    deadline: CheckDeadline,
) -> Result<InstallSource, OrchestratorError> {
    let source = read_install_formula(root, channel, identity, event, deadline)?;
    verify_install_release(source, channel, deadline)
}
fn read_install_formula(
    root: &Path,
    channel: PackageReleaseChannel,
    identity: &crate::internal_plan::native_homebrew_checks::NativeCheckIdentityExtension,
    event: velnor_actions_contract::WorkflowEvent,
    deadline: CheckDeadline,
) -> Result<AuthorityFormula, OrchestratorError> {
    use crate::internal::internal;

    if identity.operation != NativeHomebrewOperation::Install
        || identity.formula_channel != Some(channel)
    {
        return Err(internal("native_check_install_identity"));
    }
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
    if authority.tap_repository_id() != identity.tap_repository_id
        || authority.tap_base_sha() != identity.tap_base_sha
    {
        return Err(internal("native_check_protected_authority_identity"));
    }
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
    let crate::safe_read::RepoBytes::Bytes(bytes) =
        crate::safe_read::read_repo_bytes_until(root, path, 1024 * 1024, deadline)?
    else {
        return Err(internal("native_check_formula_missing"));
    };
    Ok(AuthorityFormula {
        authority,
        path: path.to_owned(),
        bytes,
    })
}

fn verify_install_release(
    source: AuthorityFormula,
    channel: PackageReleaseChannel,
    deadline: CheckDeadline,
) -> Result<InstallSource, OrchestratorError> {
    use crate::internal::internal;

    let authority = source.authority;
    let formula_bytes = source.bytes;
    let mut release_plan = authority.plan().clone();
    release_plan.channel = channel;
    let projection = validate_homebrew_projection(authority.spec(), &release_plan)
        .map_err(|_| internal("native_check_formula_projection"))?;
    let parsed = parse_generated_formula(authority.spec(), &projection, &formula_bytes)
        .map_err(|_| internal("native_check_formula_invalid"))?;
    if parsed.is_disabled_stable_sentinel() {
        return Err(internal("native_check_install_disabled_formula"));
    }
    let source_sha = install_formula_source_sha(channel, &authority, &parsed)?;
    let tag = install_formula_tag(channel, &parsed, &source_sha)?;
    if parsed.source_sha() != Some(source_sha.as_str()) || parsed.tag() != Some(tag.as_str()) {
        return Err(internal("native_check_formula_selector_mismatch"));
    }
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
    let literals = install_release_literals(&authority, &projection, &release)?;
    if render_homebrew_formula(authority.spec(), &literals)
        .map_err(|_| internal("native_check_formula_render"))?
        != formula_bytes
    {
        return Err(internal("native_check_formula_release_mismatch"));
    }
    let receipt_digest = digest_b3(
        &release
            .receipt()
            .canonical_bytes()
            .map_err(|_| internal("native_check_release_receipt_encode"))?,
    );
    Ok(InstallSource {
        authority,
        formula_path: source.path,
        formula_bytes,
        projection,
        literals,
        release,
        receipt_digest,
    })
}

fn install_formula_source_sha(
    channel: PackageReleaseChannel,
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    parsed: &HomebrewFormulaReleaseLiterals,
) -> Result<String, OrchestratorError> {
    match channel {
        PackageReleaseChannel::Preview => Ok(authority.authority_sha().to_owned()),
        PackageReleaseChannel::Stable => parsed
            .source_sha()
            .map(str::to_owned)
            .ok_or_else(|| crate::internal::internal("native_check_stable_source_sha_missing")),
    }
}

fn install_formula_tag(
    channel: PackageReleaseChannel,
    parsed: &HomebrewFormulaReleaseLiterals,
    source_sha: &str,
) -> Result<String, OrchestratorError> {
    match channel {
        PackageReleaseChannel::Preview => Ok(format!("preview-{source_sha}")),
        PackageReleaseChannel::Stable => parsed
            .tag()
            .map(str::to_owned)
            .ok_or_else(|| crate::internal::internal("native_check_stable_tag_missing")),
    }
}

fn install_release_literals(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    projection: &velnor_actions_contract::homebrew_spec::ValidatedHomebrewProjection,
    release: &crate::package_release_public::VerifiedPublicPackageRelease,
) -> Result<HomebrewFormulaReleaseLiterals, OrchestratorError> {
    use crate::internal::internal;

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
