// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, path::Path};

use velnor_actions_contract::{
    homebrew_adoption::HomebrewAdoptionTransition,
    package_release_policy::MAX_CARGO_VERSION_MANIFEST_BYTES,
};
use velnor_actions_mise::CheckDeadline;

use super::{HomebrewAdoptionAuthoringReport, output};
use crate::{OrchestratorError, internal::internal};

pub(super) const MAX_SDK_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;
type VerifiedBaseInventory = BTreeMap<String, (u32, String)>;

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
    let transition = load_transition(transition_id)?;
    let manifest_bytes =
        crate::safe_read::read_event_file(manifest_path, MAX_SDK_MANIFEST_BYTES)?.into_bytes();
    let sdk = authenticate_sdk(&transition, &manifest_bytes, deadline)?;
    let base_sha = protected_base_tip(&transition, deadline)?;
    let (files_by_path, claude) = read_seed_base(&transition, &base_sha, deadline)?;
    ensure_locator_absent(&transition, &base_sha, &files_by_path, deadline)?;
    let files = render_seed_files(&transition, &sdk, &manifest_bytes, claude)?;
    ensure_base_tip(&transition, &base_sha, deadline)?;
    output::write_overlay(
        &parent,
        &target,
        &transition.transition_id,
        &transition.base_sha,
        &transition.base_tree,
        &files,
        transition.deletions.clone(),
    )
}

fn protected_base_tip(
    transition: &HomebrewAdoptionTransition,
    deadline: CheckDeadline,
) -> Result<String, OrchestratorError> {
    let base_sha = crate::package_release_publish::read_public_branch_tip(
        &transition.tap_repository,
        transition.tap_repository_id,
        "main",
        deadline,
    )?;
    if base_sha != transition.base_sha {
        return Err(internal("homebrew_adoption_protected_base_shifted"));
    }
    Ok(base_sha)
}

fn read_seed_base(
    transition: &HomebrewAdoptionTransition,
    base_sha: &str,
    deadline: CheckDeadline,
) -> Result<(VerifiedBaseInventory, Vec<u8>), OrchestratorError> {
    let symlink_sha =
        crate::release_policy_admission::git_blob_sha1(transition.claude_source_path.as_bytes())?;
    let tree = crate::package_release_publish::read_protected_adoption_seed_base_tree_identity(
        &transition.tap_repository,
        transition.tap_repository_id,
        base_sha,
        "CLAUDE.md",
        &symlink_sha,
        deadline,
    )?;
    let inventory = verified_base_inventory(transition, &tree)?;
    let claude = crate::package_release_publish::read_protected_source_file(
        &transition.tap_repository,
        transition.tap_repository_id,
        "main",
        base_sha,
        &transition.claude_source_path,
        MAX_TAP_AGENTS_BYTES,
        deadline,
    )?;
    if crate::release_policy_admission::git_blob_sha1(&claude)? != transition.claude_source_blob_sha
    {
        return Err(internal("homebrew_adoption_claude_source_mismatch"));
    }
    Ok((inventory, claude))
}

pub(super) const MAX_TAP_AGENTS_BYTES: usize = MAX_CARGO_VERSION_MANIFEST_BYTES;

fn ensure_locator_absent(
    transition: &HomebrewAdoptionTransition,
    base_sha: &str,
    inventory: &VerifiedBaseInventory,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let locator_path =
        velnor_actions_contract::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH;
    if inventory.contains_key(locator_path)
        || crate::package_release_publish::try_read_protected_source_file(
            &transition.tap_repository,
            transition.tap_repository_id,
            "main",
            base_sha,
            locator_path,
            16 * 1024,
            deadline,
        )?
        .is_some()
    {
        return Err(internal("homebrew_adoption_locator_already_present"));
    }
    Ok(())
}

fn render_seed_files(
    transition: &HomebrewAdoptionTransition,
    sdk: &crate::homebrew::sdk_binding::PublishedSdkBindingIdentity,
    manifest_bytes: &[u8],
    claude: Vec<u8>,
) -> Result<BTreeMap<String, (u32, Vec<u8>)>, OrchestratorError> {
    let locator_path =
        velnor_actions_contract::homebrew_authority::HOMEBREW_SOURCE_AUTHORITY_LOCATOR_PATH;
    let rendered = velnor_actions_workflow_renderer::render_homebrew_adoption_seed(
        transition,
        &velnor_actions_workflow_renderer::HomebrewAdoptionSeedSdkIdentity {
            source_sha: &sdk.source_sha,
            version: &sdk.version,
            manifest_bytes,
            manifest_sha256: &sdk.manifest_sha256,
            host_target_sha256: &sdk.seed_runner_binary_sha256,
        },
        &crate::product_release_pins::homebrew_adoption_pins()?,
    )
    .map_err(|_| internal("homebrew_adoption_seed_render_invalid"))?;
    let mut files = rendered
        .into_iter()
        .map(|file| (file.path, (0o100_644, file.bytes.into_bytes())))
        .collect::<BTreeMap<_, _>>();
    files.insert("CLAUDE.md".to_owned(), (0o100_644, claude));
    files.insert(
        locator_path.to_owned(),
        (
            0o100_644,
            transition
                .source_authority_locator
                .canonical_bytes()
                .map_err(crate::internal::internal_contract)?,
        ),
    );
    Ok(files)
}

fn ensure_base_tip(
    transition: &HomebrewAdoptionTransition,
    base_sha: &str,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    let final_tip = crate::package_release_publish::read_public_branch_tip(
        &transition.tap_repository,
        transition.tap_repository_id,
        "main",
        deadline,
    )?;
    if final_tip != base_sha {
        return Err(internal("homebrew_adoption_protected_base_changed"));
    }
    Ok(())
}

pub(super) fn load_transition(
    transition_id: &str,
) -> Result<HomebrewAdoptionTransition, OrchestratorError> {
    let transition =
        crate::homebrew::adoption::ProtectedHomebrewAdoptionTransition::from_protected_source(
            include_bytes!("../../../.velnor/qualification/homebrew/jackin/transition.json"),
        )?;
    if transition.transition().transition_id != transition_id {
        return Err(internal("homebrew_adoption_transition_unknown"));
    }
    Ok(transition.transition().clone())
}

pub(super) fn authenticate_sdk(
    transition: &HomebrewAdoptionTransition,
    manifest_bytes: &[u8],
    deadline: CheckDeadline,
) -> Result<crate::homebrew::sdk_binding::PublishedSdkBindingIdentity, OrchestratorError> {
    let text = std::str::from_utf8(manifest_bytes)
        .map_err(|_| internal("homebrew_adoption_sdk_manifest_utf8"))?;
    let manifest = velnor_actions_contract::ReleaseManifest::parse_json_with_limit(
        text,
        "release-manifest.json",
        usize::try_from(MAX_SDK_MANIFEST_BYTES)
            .map_err(|_| internal("homebrew_adoption_sdk_manifest_size"))?,
    )
    .map_err(|_| internal("homebrew_adoption_sdk_manifest_invalid"))?;
    manifest
        .validate("release-manifest.json")
        .map_err(|_| internal("homebrew_adoption_sdk_manifest_invalid"))?;
    let digest = sha256(manifest_bytes);
    let admitted = crate::homebrew::adoption::protected_homebrew_adoption_transition(
        &transition.tap_repository,
        transition.tap_repository_id,
        &transition.base_sha,
    )?;
    let binding = crate::homebrew::sdk_binding::verify_published_sdk_binding(
        &admitted,
        &manifest.version,
        &manifest.commit,
        &digest,
        deadline,
    )?;
    if binding.manifest_bytes() != manifest_bytes {
        return Err(internal("homebrew_adoption_sdk_manifest_bytes_mismatch"));
    }
    Ok(binding.identity().clone())
}

fn verified_base_inventory(
    transition: &HomebrewAdoptionTransition,
    tree: &crate::package_release_publish::ProtectedSourceTreeIdentity,
) -> Result<VerifiedBaseInventory, OrchestratorError> {
    let symlink_sha =
        crate::release_policy_admission::git_blob_sha1(transition.claude_source_path.as_bytes())?;
    let regular = tree
        .regular_files
        .iter()
        .map(|entry| BaseTreeEntry {
            path: &entry.path,
            mode: &entry.mode,
            sha: &entry.sha,
            kind: "blob",
        })
        .collect::<Vec<_>>();
    let nonregular = tree
        .nonregular_entries
        .iter()
        .map(|entry| BaseTreeEntry {
            path: &entry.path,
            mode: &entry.mode,
            sha: &entry.sha,
            kind: "blob",
        })
        .collect::<Vec<_>>();
    let directories = tree
        .directories
        .iter()
        .map(|entry| BaseTreeEntry {
            path: &entry.path,
            mode: &entry.mode,
            sha: &entry.sha,
            kind: "tree",
        })
        .collect::<Vec<_>>();
    validated_base_entries(
        transition,
        &tree.tree_sha,
        &regular,
        &nonregular,
        &directories,
        &symlink_sha,
    )
}

#[derive(Clone, Copy)]
pub(super) struct BaseTreeEntry<'a> {
    pub(super) path: &'a str,
    pub(super) mode: &'a str,
    pub(super) sha: &'a str,
    pub(super) kind: &'a str,
}

pub(super) fn validated_base_entries(
    transition: &HomebrewAdoptionTransition,
    tree_sha: &str,
    regular: &[BaseTreeEntry<'_>],
    nonregular: &[BaseTreeEntry<'_>],
    directories: &[BaseTreeEntry<'_>],
    expected_symlink_sha: &str,
) -> Result<VerifiedBaseInventory, OrchestratorError> {
    if tree_sha != transition.base_tree {
        return Err(internal("homebrew_adoption_base_tree_mismatch"));
    }
    let complete = regular
        .iter()
        .chain(nonregular)
        .chain(directories)
        .map(|entry| crate::homebrew::tree_validation::Entry {
            path: entry.path,
            mode: entry.mode,
            kind: entry.kind,
            sha: entry.sha,
        })
        .collect::<Vec<_>>();
    crate::homebrew::tree_validation::validate_protected_symlink_base(
        &complete,
        100_000,
        "CLAUDE.md",
        expected_symlink_sha,
    )?;
    if nonregular.len() != 1
        || nonregular[0].path != "CLAUDE.md"
        || nonregular[0].mode != "120000"
        || nonregular[0].kind != "blob"
        || nonregular[0].sha != expected_symlink_sha
    {
        return Err(internal("homebrew_adoption_base_tree_mismatch"));
    }
    let inventory = regular
        .iter()
        .map(|entry| {
            if !matches!(entry.mode, "100644" | "100755") || entry.kind != "blob" {
                return Err(internal("homebrew_adoption_base_file_mode"));
            }
            let mode = u32::from_str_radix(entry.mode, 8)
                .map_err(|_| internal("homebrew_adoption_base_file_mode"))?;
            Ok((entry.path.to_owned(), (mode, entry.sha.to_owned())))
        })
        .collect::<Result<BTreeMap<_, _>, OrchestratorError>>()?;
    for identity in transition
        .deletions
        .iter()
        .chain(&transition.unchanged_inert_files)
    {
        if inventory
            .get(&identity.path)
            .is_none_or(|(mode, sha)| *mode != identity.mode || sha != &identity.blob_sha)
        {
            return Err(internal("homebrew_adoption_base_file_identity"));
        }
    }
    if inventory.get(&transition.claude_source_path)
        != Some(&(0o100_644, transition.claude_source_blob_sha.clone()))
    {
        return Err(internal("homebrew_adoption_claude_source_identity"));
    }
    Ok(inventory)
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(digest.len() * 2);
    for byte in digest {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}
