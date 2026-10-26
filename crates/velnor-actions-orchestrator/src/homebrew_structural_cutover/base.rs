// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize};
use velnor_actions_mise::CheckDeadline;

use crate::{OrchestratorError, internal::internal};

const CALLER_WORKFLOW: &str = ".github/workflows/ci.yml";
const MAX_CALLER_WORKFLOW_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HomebrewSeedOutputBaseIdentity {
    pub(crate) schema: String,
    pub(crate) transition_id: String,
    pub(crate) transition_digest: String,
    pub(crate) transition_base_sha: String,
    pub(crate) transition_base_tree: String,
    pub(crate) seed_output_base_sha: String,
    pub(crate) seed_output_base_tree: String,
    pub(crate) sdk_source_sha: String,
    pub(crate) sdk_version: String,
    pub(crate) sdk_manifest_sha256: String,
    pub(crate) sdk_seed_runner_binary_sha256: String,
    pub(crate) formula_spec_digest: String,
    pub(crate) witness_digest: String,
}

#[derive(Debug, Clone)]
pub(crate) struct PrivateVerifiedSeedOutputBase {
    identity: HomebrewSeedOutputBaseIdentity,
    seed_files: BTreeMap<String, Vec<u8>>,
    sdk_manifest: Vec<u8>,
}

impl PrivateVerifiedSeedOutputBase {
    pub(crate) fn identity(&self) -> &HomebrewSeedOutputBaseIdentity {
        &self.identity
    }

    pub(crate) fn seed_files(&self) -> &BTreeMap<String, Vec<u8>> {
        &self.seed_files
    }

    pub(crate) fn sdk_manifest(&self) -> &[u8] {
        &self.sdk_manifest
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SeedSelection {
    transition_id: String,
    version: String,
    source_sha: String,
    manifest_sha256: String,
}
pub(crate) fn verify_seed_output_base(
    root: &Path,
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    deadline: CheckDeadline,
) -> Result<Option<PrivateVerifiedSeedOutputBase>, OrchestratorError> {
    let transition = protected_transition()?;
    if !authority_matches_transition(authority, transition.transition()) {
        return Ok(None);
    }
    let Some(selection) = protected_selection(authority, transition.transition(), deadline)? else {
        return Ok(None);
    };
    replay_seed_output(root, authority, &transition, &selection, deadline).map(Some)
}

pub(crate) fn protected_transition()
-> Result<crate::homebrew::adoption::ProtectedHomebrewAdoptionTransition, OrchestratorError> {
    crate::homebrew::adoption::ProtectedHomebrewAdoptionTransition::from_protected_source(
        include_bytes!("../../../../.velnor/qualification/homebrew/jackin/transition.json"),
    )
}

fn authority_matches_transition(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
) -> bool {
    authority.tap().repository == transition.tap_repository
        && authority.tap_repository_id() == transition.tap_repository_id
}

fn protected_selection(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
    deadline: CheckDeadline,
) -> Result<Option<SeedSelection>, OrchestratorError> {
    let caller =
        authority.protected_tap_file(CALLER_WORKFLOW, MAX_CALLER_WORKFLOW_BYTES, deadline)?;
    let Some(selection) = parse_seed_selection(&caller)? else {
        return Ok(None);
    };
    if selection.transition_id != transition.transition_id {
        return Err(internal("homebrew_seed_output_transition_mismatch"));
    }
    Ok(Some(selection))
}

fn replay_seed_output(
    root: &Path,
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    transition: &crate::homebrew::adoption::ProtectedHomebrewAdoptionTransition,
    selection: &SeedSelection,
    deadline: CheckDeadline,
) -> Result<PrivateVerifiedSeedOutputBase, OrchestratorError> {
    let policy = transition.transition();
    let admitted = crate::homebrew::adoption::protected_homebrew_adoption_transition(
        &policy.tap_repository,
        policy.tap_repository_id,
        &policy.base_sha,
    )?;
    let binding = crate::homebrew::sdk_binding::verify_published_sdk_binding(
        &admitted,
        &selection.version,
        &selection.source_sha,
        &selection.manifest_sha256,
        deadline,
    )?;
    let generated = render_seed_files(policy, &binding)?;
    let evidence = crate::homebrew::adoption::validate_homebrew_adoption_output_base(
        root,
        &admitted,
        &generated,
        authority.tap_base_sha(),
        deadline,
    )?;
    if evidence.head_sha != authority.tap_base_sha() {
        return Err(internal("homebrew_seed_output_base_sha_mismatch"));
    }
    build_seed_witness(authority, policy, &binding, evidence, generated)
}

fn render_seed_files(
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
    binding: &crate::homebrew::sdk_binding::VerifiedPublishedSdkBinding,
) -> Result<BTreeMap<String, Vec<u8>>, OrchestratorError> {
    let sdk = binding.identity();
    let rendered = velnor_actions_workflow_renderer::render_homebrew_adoption_seed(
        transition,
        &velnor_actions_workflow_renderer::HomebrewAdoptionSeedSdkIdentity {
            source_sha: &sdk.source_sha,
            version: &sdk.version,
            manifest_bytes: binding.manifest_bytes(),
            manifest_sha256: &sdk.manifest_sha256,
            host_target_sha256: &sdk.seed_runner_binary_sha256,
        },
        &crate::product_release_pins::homebrew_adoption_pins()?,
    )
    .map_err(|_| internal("homebrew_seed_output_render_invalid"))?;
    Ok(rendered
        .into_iter()
        .map(|file| (file.path, file.bytes.into_bytes()))
        .collect())
}

fn build_seed_witness(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    transition: &velnor_actions_contract::homebrew_adoption::HomebrewAdoptionTransition,
    binding: &crate::homebrew::sdk_binding::VerifiedPublishedSdkBinding,
    evidence: velnor_actions_contract::homebrew_adoption::HomebrewAdoptionEvidence,
    generated: BTreeMap<String, Vec<u8>>,
) -> Result<PrivateVerifiedSeedOutputBase, OrchestratorError> {
    let sdk = binding.identity();
    let formula_spec_digest = authority
        .spec()
        .digest()
        .map_err(crate::internal::internal_contract)?;
    let preimage = SeedOutputBaseWitnessPreimage {
        transition_id: transition.transition_id.clone(),
        transition_digest: evidence.transition_digest,
        transition_base_sha: evidence.base_sha,
        transition_base_tree: evidence.base_tree,
        seed_output_base_sha: evidence.head_sha,
        seed_output_base_tree: evidence.head_tree,
        sdk_source_sha: sdk.source_sha.clone(),
        sdk_version: sdk.version.clone(),
        sdk_manifest_sha256: sdk.manifest_sha256.clone(),
        sdk_seed_runner_binary_sha256: sdk.seed_runner_binary_sha256.clone(),
        formula_spec_digest: formula_spec_digest.clone(),
    };
    let witness_digest = velnor_actions_contract::digest_b3(
        &velnor_actions_contract::canonical_json_bytes(&preimage)
            .map_err(crate::internal::internal_contract)?,
    );
    let identity = HomebrewSeedOutputBaseIdentity {
        schema: "homebrew-seed-output-base-v1".to_owned(),
        transition_id: preimage.transition_id,
        transition_digest: preimage.transition_digest,
        transition_base_sha: preimage.transition_base_sha,
        transition_base_tree: preimage.transition_base_tree,
        seed_output_base_sha: preimage.seed_output_base_sha,
        seed_output_base_tree: preimage.seed_output_base_tree,
        sdk_source_sha: preimage.sdk_source_sha,
        sdk_version: preimage.sdk_version,
        sdk_manifest_sha256: preimage.sdk_manifest_sha256,
        sdk_seed_runner_binary_sha256: preimage.sdk_seed_runner_binary_sha256,
        formula_spec_digest,
        witness_digest,
    };
    Ok(PrivateVerifiedSeedOutputBase {
        identity,
        seed_files: generated,
        sdk_manifest: binding.manifest_bytes().to_vec(),
    })
}

fn parse_seed_selection(bytes: &[u8]) -> Result<Option<SeedSelection>, OrchestratorError> {
    let invalid = || internal("homebrew_seed_output_caller_invalid");
    if bytes.is_empty() || bytes.len() > MAX_CALLER_WORKFLOW_BYTES {
        return Err(invalid());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    let lines = text.lines().collect::<Vec<_>>();
    let Some(section) = unique_line(&lines, "  homebrew-adoption:")? else {
        return Ok(None);
    };
    let values = seed_selection_values(&lines, section)?;
    let workflow_prefix = "tailrocks/velnor-new/.github/workflows/homebrew-adoption.yml@";
    let pinned_source = values[0]
        .strip_prefix(workflow_prefix)
        .ok_or_else(invalid)?;
    let transition_id = &values[1];
    let version = &values[2];
    let source_sha = &values[3];
    let manifest_sha256 = &values[4];
    if !valid_sha(pinned_source)
        || pinned_source != source_sha
        || !velnor_actions_contract::config::is_exact_semver(version)
        || !valid_sha(source_sha)
        || !valid_sha256(manifest_sha256)
    {
        return Err(invalid());
    }
    Ok(Some(SeedSelection {
        transition_id: transition_id.clone(),
        version: version.clone(),
        source_sha: source_sha.clone(),
        manifest_sha256: manifest_sha256.clone(),
    }))
}

fn seed_selection_values(lines: &[&str], section: usize) -> Result<Vec<String>, OrchestratorError> {
    let invalid = || internal("homebrew_seed_output_caller_invalid");
    let expected_keys = [
        "    uses: ",
        "    with:",
        "      transition_id: ",
        "      sdk_version: ",
        "      sdk_source_sha: ",
        "      sdk_manifest_sha256: ",
    ];
    let values = expected_keys
        .iter()
        .enumerate()
        .map(|(offset, prefix)| {
            let line = lines.get(section + offset + 1).ok_or_else(invalid)?;
            let value = line.strip_prefix(prefix).ok_or_else(invalid)?;
            if prefix.ends_with(':') {
                if !value.is_empty() {
                    return Err(invalid());
                }
                Ok(None)
            } else {
                Ok(Some(parse_scalar(value).ok_or_else(invalid)?))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    if lines
        .iter()
        .filter(|line| {
            [
                "      transition_id:",
                "      sdk_version:",
                "      sdk_source_sha:",
                "      sdk_manifest_sha256:",
            ]
            .iter()
            .any(|prefix| line.starts_with(prefix))
        })
        .count()
        != 4
    {
        return Err(invalid());
    }
    let values = values.into_iter().flatten().collect::<Vec<_>>();
    if values.len() != 5 {
        return Err(invalid());
    }
    Ok(values)
}

fn unique_line(lines: &[&str], expected: &str) -> Result<Option<usize>, OrchestratorError> {
    let mut found = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (*line == expected).then_some(index));
    let first = found.next();
    if found.next().is_some() {
        return Err(internal("homebrew_seed_output_caller_duplicate"));
    }
    Ok(first)
}

fn parse_scalar(text: &str) -> Option<String> {
    let value = if text.starts_with('"') && text.ends_with('"') && text.len() >= 2 {
        &text[1..text.len() - 1]
    } else {
        text
    };
    (!value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/' | b'@')
        }))
    .then(|| value.to_owned())
}

fn valid_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[derive(Serialize)]
struct SeedOutputBaseWitnessPreimage {
    transition_id: String,
    transition_digest: String,
    transition_base_sha: String,
    transition_base_tree: String,
    seed_output_base_sha: String,
    seed_output_base_tree: String,
    sdk_source_sha: String,
    sdk_version: String,
    sdk_manifest_sha256: String,
    sdk_seed_runner_binary_sha256: String,
    formula_spec_digest: String,
}
