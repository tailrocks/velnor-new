// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::super::Plan;
use crate::homebrew::runtime::{
    NativeHomebrewHostedProfileEvidence, NativeHomebrewInstallEvidence,
};
use crate::internal_plan::native_homebrew_checks::NativeCheckIdentityExtension;

pub(super) fn validate_install(
    plan: &Plan,
    expected: &NativeCheckIdentityExtension,
    operation: velnor_actions_contract::config::NativeHomebrewOperation,
    evidence: &NativeHomebrewInstallEvidence,
    receipt_digest: &str,
    hosted: &NativeHomebrewHostedProfileEvidence,
    native_root: Option<&Path>,
) -> bool {
    let Some(root) = native_root else {
        return false;
    };
    let Some(channel) = expected.formula_channel else {
        return false;
    };
    let Some(deadline) = super::proof::proof_deadline() else {
        return false;
    };
    let Ok((Some(expected_receipt), formula_bytes)) =
        crate::homebrew::runtime::verify_formula(root, channel, expected, plan.event, deadline)
    else {
        return false;
    };
    let Ok(pull_request) =
        crate::internal_plan::native_homebrew_checks::pull_request_number_from_env(plan.event)
    else {
        return false;
    };
    let Ok(authority) = crate::homebrew::authority::load_protected_homebrew_authority(
        root,
        &expected.tap_repository,
        plan.event,
        pull_request,
        &expected.head,
        deadline,
    ) else {
        return false;
    };
    let Some(projection) = install_projection(&authority, channel) else {
        return false;
    };
    let Some((formula, version)) =
        install_formula(authority.spec(), channel, &projection, &formula_bytes)
    else {
        return false;
    };
    operation == velnor_actions_contract::config::NativeHomebrewOperation::Install
        && receipt_digest == expected_receipt
        && validate_install_evidence(
            authority.spec(),
            &projection,
            expected,
            evidence,
            &formula,
            &version,
        )
        && validate_hosted_profile(hosted)
}

fn install_projection(
    authority: &crate::homebrew::authority::VerifiedHomebrewAuthority,
    channel: velnor_actions_contract::package_release::PackageReleaseChannel,
) -> Option<velnor_actions_contract::homebrew_spec::ValidatedHomebrewProjection> {
    let mut plan = authority.plan().clone();
    plan.channel = channel;
    velnor_actions_contract::homebrew_spec::validate_homebrew_projection(authority.spec(), &plan)
        .ok()
}

fn install_formula(
    spec: &velnor_actions_contract::homebrew_spec::HomebrewFormulaSpec,
    channel: velnor_actions_contract::package_release::PackageReleaseChannel,
    projection: &velnor_actions_contract::homebrew_spec::ValidatedHomebrewProjection,
    formula_bytes: &[u8],
) -> Option<(String, String)> {
    let literals = velnor_actions_contract::homebrew_spec::parse_generated_formula(
        spec,
        projection,
        formula_bytes,
    )
    .ok()?;
    let formula = spec
        .channels
        .iter()
        .find(|item| item.channel == channel)
        .map(|item| item.formula.clone())?;
    Some((formula, literals.version().to_owned()))
}

fn validate_install_evidence(
    spec: &velnor_actions_contract::homebrew_spec::HomebrewFormulaSpec,
    projection: &velnor_actions_contract::homebrew_spec::ValidatedHomebrewProjection,
    expected: &NativeCheckIdentityExtension,
    evidence: &NativeHomebrewInstallEvidence,
    formula: &str,
    version: &str,
) -> bool {
    let Ok(files) = crate::internal_plan::native_homebrew_checks::expected_install_files(
        spec,
        projection,
        expected.platform,
    ) else {
        return false;
    };
    let expected_files = files
        .into_iter()
        .map(|file| ((file.asset.clone(), file.member.clone()), file))
        .collect::<std::collections::BTreeMap<_, _>>();
    evidence.formula == formula
        && evidence.version == version
        && evidence.brew_version == crate::homebrew::install::HOMEBREW_VERSION
        && evidence.prefix == crate::homebrew::install::HOMEBREW_PREFIX
        && evidence.cellar == "/opt/homebrew/Cellar"
        && evidence.keg == format!("/opt/homebrew/Cellar/{formula}/{version}")
        && evidence.install_time > 0
        && valid_install_digests(evidence)
        && validate_installed_files(evidence, &expected_files)
}

fn valid_install_digests(evidence: &NativeHomebrewInstallEvidence) -> bool {
    [
        evidence.verification_sha256.as_str(),
        evidence.receipt_sha256.as_str(),
        evidence.recipe_sha256.as_str(),
        evidence.payload_sha256.as_str(),
        evidence.version_output_sha256.as_str(),
    ]
    .iter()
    .all(|digest| valid_sha256(digest))
}

fn validate_installed_files(
    evidence: &NativeHomebrewInstallEvidence,
    expected: &std::collections::BTreeMap<
        (String, String),
        crate::internal_plan::native_homebrew_checks::NativeExpectedInstallFile,
    >,
) -> bool {
    let actual = evidence
        .installed_files
        .iter()
        .map(|file| (file.asset.clone(), file.member.clone()))
        .collect::<std::collections::BTreeSet<_>>();
    !expected.is_empty()
        && evidence.installed_files.len() == expected.len()
        && actual.len() == evidence.installed_files.len()
        && actual.iter().all(|key| expected.contains_key(key))
        && evidence
            .installed_files
            .iter()
            .all(|file| installed_file_matches(file, expected))
}

fn installed_file_matches(
    actual: &crate::homebrew::install::HomebrewInstalledFileEvidence,
    expected: &std::collections::BTreeMap<
        (String, String),
        crate::internal_plan::native_homebrew_checks::NativeExpectedInstallFile,
    >,
) -> bool {
    let Some(expected) = expected.get(&(actual.asset.clone(), actual.member.clone())) else {
        return false;
    };
    actual.destination == expected.destination
        && actual.mode == expected.mode
        && actual.test == expected.test
        && actual
            .elf
            .as_ref()
            .map(|facts| (facts.class, facts.endian, facts.machine))
            == expected.elf
        && valid_sha256(&actual.sha256)
}

fn validate_hosted_profile(hosted: &NativeHomebrewHostedProfileEvidence) -> bool {
    valid_git_sha(&hosted.source_commit)
        && valid_git_sha(&hosted.source_tree)
        && valid_git_sha(&hosted.source_tag_commit)
        && !hosted.source_describe.is_empty()
        && !hosted.portable_ruby_version.is_empty()
        && valid_sha256(&hosted.portable_ruby_archive_sha256)
        && valid_sha256(&hosted.portable_ruby_binary_sha256)
        && hosted.active_prefix_device > 0
        && hosted.active_prefix_inode > 0
}

fn valid_git_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
