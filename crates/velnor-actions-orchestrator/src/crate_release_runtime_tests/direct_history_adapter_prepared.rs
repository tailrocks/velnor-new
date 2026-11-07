// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) fn exercise_prepared(
    root: &std::path::Path,
    policy: &str,
    source: &str,
    request: &crate::crate_release_github::GitHubAdmissionRequest,
    receipt: &crate::crate_release_journal::JournalReceipt,
    historical: &velnor_actions_contract::config::CrateReleaseIntent,
    old_policy: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut prior = receipt.clone();
    prior.identity.workflow_sha = old_policy.to_owned();
    prior.identity.run_id = 11;
    prior.predecessor = None;
    prior.workflow_run_url = "https://github.com/owner/project/actions/runs/11".into();
    prior
        .generator_version
        .clone_from(&historical.generator_version);
    prior.expected_tools.generator_sha256 =
        "040c647a575684b84f28588862bad78876faf0224d8e5538c7a680d7d528f6b3".into();
    prior.expected_tools.release_plz_sha256 = velnor_actions_mise::catalog::qualified_release_plz(
        &historical.release_plz_version,
        &historical.runner,
    )
    .ok_or("historical release-plz pin")?
    .executable_sha256
    .into();
    prior.intent_sha256 = crate::planner::generator::sha256_hex(
        &velnor_actions_contract::canonical_json_bytes(&historical)?,
    );
    prior.base_intent_sha256 =
        crate::crate_release_journal::release_base_intent_sha256(historical)?;
    super::direct_adapter::exercise(root, policy, source, request, receipt, &prior)?;
    Ok(())
}
