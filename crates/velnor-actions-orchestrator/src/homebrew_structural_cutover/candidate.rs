//! Canonical output construction and validation for the one-time cutover.

use std::{collections::BTreeMap, path::Path};

use velnor_actions_contract::package_release::PackageReleaseChannel;
use velnor_actions_mise::{CheckDeadline, TapTreeEntry};

use crate::{OrchestratorError, internal::internal};

use super::{HomebrewSeedOutputBaseIdentity, PrivateVerifiedSeedOutputBase, protected_transition};

pub(crate) type RegularTree = BTreeMap<String, (u32, Vec<u8>)>;

/// Compare a cutover candidate against the normal generated output map.
/// The current `FormulaContract` receipt independently authenticates the preview
/// formula; that formula's verified bytes enter the reusable expected-tree factory.
pub(crate) fn validate_structural_candidate(
    root: &Path,
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    witness: &PrivateVerifiedSeedOutputBase,
    deadline: CheckDeadline,
) -> Result<(), OrchestratorError> {
    validate_witness(authority, witness)?;
    let preview = read_preview_candidate(root, authority, deadline)?;
    let expected = expected_structural_output_tree(root, authority, witness, &preview, deadline)?;
    let actual = candidate_tree(root, authority.tap_head_sha(), deadline)?;
    if !same_regular_tree(&actual, &expected) {
        return Err(internal("homebrew_cutover_candidate_tree_mismatch"));
    }
    Ok(())
}

/// Build the exact normal renderer output over the authenticated seed base.
/// Callers supply preview formula bytes after release/formula contract checks.
pub(crate) fn expected_structural_output_tree(
    root: &Path,
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    witness: &PrivateVerifiedSeedOutputBase,
    verified_preview_formula: &[u8],
    deadline: CheckDeadline,
) -> Result<RegularTree, OrchestratorError> {
    validate_witness(authority, witness)?;
    let (config, serialized_config) = normal_config(authority, witness, deadline)?;
    let base = seed_base_tree(root, witness, &config, deadline)?;
    let mut expected = preserved_seed_files(root, witness, &base, deadline)?;
    overlay_normal_renderer(root, &config, &mut expected)?;
    expected.insert(
        crate::config::CONFIG_REL.to_owned(),
        (0o100_644, serialized_config),
    );
    expected.insert(
        ".velnor/release-manifest.json".to_owned(),
        (0o100_644, witness.sdk_manifest().to_vec()),
    );
    overlay_channel_formulas(authority, verified_preview_formula, &mut expected)?;
    Ok(expected)
}

fn validate_witness(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    witness: &PrivateVerifiedSeedOutputBase,
) -> Result<(), OrchestratorError> {
    if witness.identity().seed_output_base_sha != authority.tap_base_sha()
        || witness.identity().formula_spec_digest
            != authority
                .spec()
                .digest()
                .map_err(crate::internal::internal_contract)?
    {
        return Err(internal("homebrew_cutover_witness_authority"));
    }
    Ok(())
}

fn normal_config(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    witness: &PrivateVerifiedSeedOutputBase,
    deadline: CheckDeadline,
) -> Result<(velnor_actions_contract::VelnorConfig, Vec<u8>), OrchestratorError> {
    let bytes = witness
        .seed_files()
        .get(crate::config::CONFIG_REL)
        .ok_or_else(|| internal("homebrew_cutover_seed_config_missing"))?;
    let text =
        std::str::from_utf8(bytes).map_err(|_| internal("homebrew_cutover_seed_config_utf8"))?;
    let mut config = crate::config::load_config_from_text(text)?;
    config.native_checks =
        crate::internal_plan::native_homebrew_checks::required_native_homebrew_checks(
            authority,
            Some(witness),
            deadline,
        )?;
    let serialized = toml::to_string(&config)
        .map_err(|_| internal("homebrew_cutover_config_encode"))?
        .into_bytes();
    Ok((config, serialized))
}

fn seed_base_tree(
    root: &Path,
    witness: &PrivateVerifiedSeedOutputBase,
    config: &velnor_actions_contract::VelnorConfig,
    deadline: CheckDeadline,
) -> Result<(Vec<TapTreeEntry>, Vec<String>), OrchestratorError> {
    let base = velnor_actions_mise::read_tap_tree(
        root,
        &witness.identity().seed_output_base_sha,
        deadline,
    )
    .map_err(|error| internal(&error.to_string()))?;
    validate_helper_pin(&base)?;
    let projection = materialize_seed_workflows(root, witness, &base, deadline)?;
    let unmanaged = crate::generate::unmanaged::list(
        projection.path(),
        &config.workflow.preserve_workflows,
        &velnor_actions_contract::GithubConfig::default(),
    )?;
    Ok((base, unmanaged))
}

fn validate_helper_pin(base: &[TapTreeEntry]) -> Result<(), OrchestratorError> {
    let transition = protected_transition()?;
    let pinned = transition
        .transition()
        .unchanged_inert_files
        .iter()
        .find(|file| file.path == "lib/jackin_formula_support.rb" && file.mode == 0o100_644)
        .ok_or_else(|| internal("homebrew_cutover_helper_pin_missing"))?;
    let actual = base
        .iter()
        .find(|entry| entry.path == pinned.path)
        .map(|entry| entry.blob_sha.as_str());
    if actual != Some(pinned.blob_sha.as_str()) {
        return Err(internal("homebrew_cutover_helper_pin_mismatch"));
    }
    Ok(())
}

fn materialize_seed_workflows(
    root: &Path,
    witness: &PrivateVerifiedSeedOutputBase,
    base: &[TapTreeEntry],
    deadline: CheckDeadline,
) -> Result<tempfile::TempDir, OrchestratorError> {
    let projection = tempfile::Builder::new()
        .prefix("velnor-homebrew-cutover-base-")
        .tempdir()
        .map_err(|error| OrchestratorError::io("homebrew_cutover_projection", error.to_string()))?;
    for entry in base
        .iter()
        .filter(|entry| entry.path.starts_with(".github/"))
    {
        let bytes = crate::homebrew::adoption::read_commit_file(
            root,
            &witness.identity().seed_output_base_sha,
            &entry.path,
            deadline,
        )?;
        let path = projection.path().join(&entry.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                OrchestratorError::io(parent.display().to_string(), error.to_string())
            })?;
        }
        std::fs::write(&path, bytes).map_err(|error| {
            OrchestratorError::io(path.display().to_string(), error.to_string())
        })?;
    }
    Ok(projection)
}

fn preserved_seed_files(
    root: &Path,
    witness: &PrivateVerifiedSeedOutputBase,
    base: &(Vec<TapTreeEntry>, Vec<String>),
    deadline: CheckDeadline,
) -> Result<RegularTree, OrchestratorError> {
    let mut expected = RegularTree::new();
    for entry in &base.0 {
        let generated =
            entry.path.starts_with(".github/") && !base.1.iter().any(|path| path == &entry.path);
        if generated || entry.path == "lib/jackin_formula_support.rb" {
            continue;
        }
        let bytes = crate::homebrew::adoption::read_commit_file(
            root,
            &witness.identity().seed_output_base_sha,
            &entry.path,
            deadline,
        )?;
        expected.insert(entry.path.clone(), (entry.mode, bytes));
    }
    Ok(expected)
}

fn overlay_normal_renderer(
    root: &Path,
    config: &velnor_actions_contract::VelnorConfig,
    expected: &mut RegularTree,
) -> Result<(), OrchestratorError> {
    let preparation = crate::prepare::prepare_with_config(root, config.clone())?;
    let rendered = crate::generate::render_staged_tree(&preparation)?;
    if !rendered.symlinks.is_empty() {
        return Err(internal("homebrew_cutover_generated_symlink"));
    }
    for file in rendered.files {
        expected.insert(file.path, (0o100_644, file.bytes.into_bytes()));
    }
    Ok(())
}

fn overlay_channel_formulas(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    preview: &[u8],
    expected: &mut RegularTree,
) -> Result<(), OrchestratorError> {
    for channel in &authority.spec().channels {
        let bytes = match channel.channel {
            PackageReleaseChannel::Preview => preview.to_vec(),
            PackageReleaseChannel::Stable => stable_sentinel(authority)?,
        };
        expected.insert(channel.destination.clone(), (0o100_644, bytes));
    }
    Ok(())
}

fn stable_sentinel(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
) -> Result<Vec<u8>, OrchestratorError> {
    let mut plan = authority.plan().clone();
    plan.channel = PackageReleaseChannel::Stable;
    let projection = velnor_actions_contract::homebrew_spec::validate_homebrew_projection(
        authority.spec(),
        &plan,
    )
    .map_err(|_| internal("homebrew_cutover_stable_projection"))?;
    let sentinel =
        velnor_actions_contract::homebrew_spec::HomebrewFormulaReleaseLiterals::stable_sentinel(
            authority.spec(),
            &projection,
        )
        .map_err(|_| internal("homebrew_cutover_stable_sentinel"))?;
    velnor_actions_contract::homebrew_spec::render_homebrew_formula(authority.spec(), &sentinel)
        .map_err(|_| internal("homebrew_cutover_stable_render"))
}

fn read_preview_candidate(
    root: &Path,
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    deadline: CheckDeadline,
) -> Result<Vec<u8>, OrchestratorError> {
    let path = authority
        .spec()
        .channels
        .iter()
        .find(|channel| channel.channel == PackageReleaseChannel::Preview)
        .map(|channel| channel.destination.as_str())
        .ok_or_else(|| internal("homebrew_cutover_preview_path_missing"))?;
    crate::homebrew::adoption::read_commit_file(root, authority.tap_head_sha(), path, deadline)
}

fn candidate_tree(
    root: &Path,
    head: &str,
    deadline: CheckDeadline,
) -> Result<RegularTree, OrchestratorError> {
    let tree = velnor_actions_mise::read_tap_tree(root, head, deadline)
        .map_err(|error| internal(&error.to_string()))?;
    let mut actual = RegularTree::new();
    for entry in tree {
        let bytes = crate::homebrew::adoption::read_commit_file(root, head, &entry.path, deadline)?;
        actual.insert(entry.path, (entry.mode, bytes));
    }
    Ok(actual)
}

fn same_regular_tree(actual: &RegularTree, expected: &RegularTree) -> bool {
    actual == expected
}

/// Only an exact seed base may classify its pinned legacy stable formula inert.
pub(crate) fn is_exact_legacy_stable_formula(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    witness: &PrivateVerifiedSeedOutputBase,
    deadline: CheckDeadline,
) -> Result<bool, OrchestratorError> {
    let transition = protected_transition()?;
    if !legacy_witness_matches(authority, witness, transition.transition())? {
        return Ok(false);
    }
    compare_legacy_stable(authority, transition.transition(), deadline)
}

fn legacy_witness_matches(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    witness: &PrivateVerifiedSeedOutputBase,
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
) -> Result<bool, OrchestratorError> {
    let identity: &HomebrewSeedOutputBaseIdentity = witness.identity();
    Ok(identity.seed_output_base_sha == authority.tap_base_sha()
        && identity.transition_id == transition.transition_id
        && identity.transition_digest
            == transition
                .digest()
                .map_err(crate::internal::internal_contract)?
        && identity.transition_base_sha == transition.base_sha
        && identity.transition_base_tree == transition.base_tree
        && identity.formula_spec_digest
            == authority
                .spec()
                .digest()
                .map_err(crate::internal::internal_contract)?)
}

fn compare_legacy_stable(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
    deadline: CheckDeadline,
) -> Result<bool, OrchestratorError> {
    let Some(stable) = authority
        .spec()
        .channels
        .iter()
        .find(|channel| channel.channel == PackageReleaseChannel::Stable)
    else {
        return Ok(false);
    };
    let Some(pinned) = transition
        .unchanged_inert_files
        .iter()
        .find(|file| file.path == stable.destination && file.mode == 0o100_644)
    else {
        return Ok(false);
    };
    let bytes = authority.protected_tap_file(&stable.destination, 1024 * 1024, deadline)?;
    let blob = crate::release_policy_admission::git_blob_sha1(&bytes)
        .map_err(|_| internal("homebrew_seed_output_stable_blob_digest"))?;
    Ok(blob == pinned.blob_sha)
}

#[cfg(test)]
#[test]
fn structural_tree_comparison_rejects_additions_deletions_modes_and_bytes() {
    let expected = BTreeMap::from([
        (
            ".github/workflows/ci.yml".to_owned(),
            (0o100_644, b"workflow".to_vec()),
        ),
        (
            "Formula/jackin-preview.rb".to_owned(),
            (0o100_644, b"preview".to_vec()),
        ),
    ]);
    assert!(same_regular_tree(&expected, &expected));
    let mut changed = expected.clone();
    changed.insert(
        "Formula/jackin.rb".to_owned(),
        (0o100_644, b"extra".to_vec()),
    );
    assert!(!same_regular_tree(&changed, &expected));
    changed = expected.clone();
    changed.remove(".github/workflows/ci.yml");
    assert!(!same_regular_tree(&changed, &expected));
    changed = expected.clone();
    changed.insert(
        "Formula/jackin-preview.rb".to_owned(),
        (0o100_755, b"preview".to_vec()),
    );
    assert!(!same_regular_tree(&changed, &expected));
    changed.insert(
        "Formula/jackin-preview.rb".to_owned(),
        (0o100_644, b"tampered".to_vec()),
    );
    assert!(!same_regular_tree(&changed, &expected));
}
