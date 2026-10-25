use std::{collections::BTreeMap, path::Path};

use velnor_actions_contract::{WorkflowEvent, package_release::PackageReleaseChannel};
use velnor_actions_mise::CheckDeadline;

use super::{HomebrewAdoptionAuthoringReport, output, seed};
use crate::{OrchestratorError, internal::internal};

const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;
type OverlayFiles = BTreeMap<String, (u32, Vec<u8>)>;
type DeletedFiles = Vec<velnor_actions_contract::homebrew_adoption::HomebrewAdoptionFileIdentity>;
type CutoverOverlay = (String, OverlayFiles, DeletedFiles);
type TreeDiff = (OverlayFiles, DeletedFiles);

pub(super) fn author(
    transition_id: &str,
    manifest_path: &Path,
    output_dir: &Path,
    deadline: CheckDeadline,
) -> Result<HomebrewAdoptionAuthoringReport, OrchestratorError> {
    if !manifest_path.is_absolute() {
        return Err(internal("homebrew_adoption_sdk_manifest_path"));
    }
    let root = crate::root::resolve_root(
        &std::env::current_dir().map_err(|_| internal("homebrew_adoption_current_directory"))?,
    )?;
    let (parent, target) = output::fresh_output_path(output_dir, &root)?;
    let transition = seed::load_transition(transition_id)?;
    let manifest =
        crate::safe_read::read_event_file(manifest_path, MAX_MANIFEST_BYTES)?.into_bytes();
    let manifest_identity = seed::authenticate_sdk(&transition, &manifest, deadline)?;
    let tip = crate::package_release_publish::read_public_branch_tip(
        &transition.tap_repository,
        transition.tap_repository_id,
        "main",
        deadline,
    )?;
    let authority = crate::homebrew::authority::load_protected_homebrew_authority(
        &root,
        &transition.tap_repository,
        WorkflowEvent::Push,
        None,
        &tip,
        deadline,
    )?;
    validate_authority(&authority, &transition, &tip)?;
    let witness =
        verified_seed_witness(&root, &authority, &transition, &manifest_identity, deadline)?;
    let (base_tree, files, deleted) =
        build_cutover_overlay(&root, &authority, &transition, &witness, deadline)?;
    let final_tip = crate::package_release_publish::read_public_branch_tip(
        &transition.tap_repository,
        transition.tap_repository_id,
        "main",
        deadline,
    )?;
    if final_tip != tip {
        return Err(internal("homebrew_adoption_cutover_tip_changed"));
    }
    let final_authority = crate::homebrew::authority::load_protected_homebrew_authority(
        &root,
        &transition.tap_repository,
        WorkflowEvent::Push,
        None,
        &tip,
        deadline,
    )?;
    if final_authority.authority_sha() != authority.authority_sha()
        || final_authority.tap_base_sha() != authority.tap_base_sha()
    {
        return Err(internal("homebrew_adoption_cutover_authority_changed"));
    }
    output::write_overlay(
        &parent,
        &target,
        &transition.transition_id,
        &tip,
        &base_tree,
        &files,
        deleted,
    )
}

fn validate_authority(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
    tip: &str,
) -> Result<(), OrchestratorError> {
    if authority.tap_head_sha() != tip
        || authority.tap().repository != transition.tap_repository
        || authority.tap_repository_id() != transition.tap_repository_id
    {
        return Err(internal("homebrew_adoption_cutover_authority_mismatch"));
    }
    Ok(())
}

fn verified_seed_witness(
    root: &Path,
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
    manifest: &crate::homebrew::sdk_binding::PublishedSdkBindingIdentity,
    deadline: CheckDeadline,
) -> Result<crate::homebrew::structural_cutover::PrivateVerifiedSeedOutputBase, OrchestratorError> {
    let witness =
        crate::homebrew::structural_cutover::verify_seed_output_base(root, authority, deadline)?
            .ok_or_else(|| internal("homebrew_adoption_seed_base_missing"))?;
    let identity = witness.identity();
    if identity.transition_id != transition.transition_id
        || identity.sdk_source_sha != manifest.source_sha
        || identity.sdk_version != manifest.version
        || identity.sdk_manifest_sha256 != manifest.manifest_sha256
        || identity.sdk_seed_runner_binary_sha256 != manifest.seed_runner_binary_sha256
    {
        return Err(internal("homebrew_adoption_cutover_seed_binding_mismatch"));
    }
    Ok(witness)
}

fn build_cutover_overlay(
    root: &Path,
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
    witness: &crate::homebrew::structural_cutover::PrivateVerifiedSeedOutputBase,
    deadline: CheckDeadline,
) -> Result<CutoverOverlay, OrchestratorError> {
    let preview = authenticated_preview_formula(authority, deadline)?;
    let expected = crate::homebrew::structural_cutover::expected_structural_output_tree(
        root, authority, witness, &preview, deadline,
    )?;
    let base = crate::package_release_publish::read_protected_source_tree_identity(
        &transition.tap_repository,
        transition.tap_repository_id,
        authority.tap_base_sha(),
        deadline,
    )?;
    if base.tree_sha != witness.identity().seed_output_base_tree
        || !base.nonregular_entries.is_empty()
    {
        return Err(internal("homebrew_adoption_cutover_base_tree_mismatch"));
    }
    let base_files = base
        .regular_files
        .iter()
        .map(|entry| {
            let mode = u32::from_str_radix(&entry.mode, 8)
                .map_err(|_| internal("homebrew_adoption_cutover_base_mode"))?;
            Ok((entry.path.clone(), (mode, entry.sha.clone())))
        })
        .collect::<Result<BTreeMap<_, _>, OrchestratorError>>()?;
    let (files, deleted) = diff_tree(&base_files, &expected)?;
    Ok((base.tree_sha, files, deleted))
}

fn authenticated_preview_formula(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    let _channel = authority
        .spec()
        .channels
        .iter()
        .find(|channel| channel.channel == PackageReleaseChannel::Preview)
        .ok_or_else(|| internal("homebrew_adoption_preview_channel_missing"))?;
    crate::homebrew::runtime::render_current_preview_formula(authority, deadline)
        .map(|(_, formula)| formula)
}

fn diff_tree(
    base: &BTreeMap<String, (u32, String)>,
    expected: &crate::homebrew::structural_cutover::RegularTree,
) -> Result<TreeDiff, OrchestratorError> {
    let mut files = BTreeMap::new();
    for (path, (mode, bytes)) in expected {
        let blob = crate::release_policy_admission::git_blob_sha1(bytes)?;
        if base.get(path) != Some(&(*mode, blob)) {
            files.insert(path.clone(), (*mode, bytes.clone()));
        }
    }
    let mut deleted = Vec::new();
    for (path, (mode, blob_sha)) in base {
        if !expected.contains_key(path) {
            deleted.push(
                velnor_actions_contract::homebrew_adoption::HomebrewAdoptionFileIdentity {
                    path: path.clone(),
                    mode: *mode,
                    blob_sha: blob_sha.clone(),
                },
            );
        }
    }
    Ok((files, deleted))
}
