// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{CrateJobInputs, build_jobs::cap_cargo_jobs};
use crate::{OrchestratorError, internal::internal};
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract::{CrateJob, CrateObligation, Step, WorkflowPolicy};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

pub(super) fn prepare_crate_tools_steps(
    input: &CrateJobInputs<'_>,
    suite: Option<crate::matrix_step::CrateSuite>,
    use_rust: bool,
    use_nextest: bool,
    use_opentofu: bool,
) -> Result<Vec<Step>, OrchestratorError> {
    let needs_validators = crate::matrix_step::crate_needs_generate_validators(input.policy, suite);
    let needs_bun = input.policy == WorkflowPolicy::VelnorRepositoryV1
        && suite.is_some_and(crate::matrix_step::CrateSuite::owns_bun);
    let needs_crane = input.policy == WorkflowPolicy::VelnorRepositoryV1
        && suite.is_some_and(crate::matrix_step::CrateSuite::owns_crane);
    let mut steps = vec![crate::matrix_step::prepare_crate_tools_step_with_bun(
        input.catalog,
        use_rust,
        use_nextest,
        crate::matrix_step::prepare_install_opentofu(input.policy, suite, use_opentofu),
        needs_validators,
        needs_bun,
        needs_crane,
    )?];
    let verify = needs_validators.then(|| crate::workflow::verify_jactionlint_step(input.catalog));
    steps.extend(verify);
    Ok(steps)
}

pub(super) fn image_publication_fixture_step(
    input: &CrateJobInputs<'_>,
    model: &CrateJob,
) -> Result<Vec<Step>, OrchestratorError> {
    if input.policy != WorkflowPolicy::VelnorRepositoryV1
        || model.package_name != "velnor-actions-cli"
    {
        return Ok(Vec::new());
    }
    let env = crate::matrix_step::task_step_env(
        input.catalog,
        &BTreeMap::new(),
        true,
    )?;
    let build = mbx_image_fixture_argv(
        input.catalog,
        &[
            "build",
            "--locked",
            "-p",
            "velnor-actions-cli",
            "--example",
            "image-publication-fixture",
            "--message-format=json",
        ],
    )?;
    let resolve = mbx_image_fixture_argv(
        input.catalog,
        &[
            "run",
            "--quiet",
            "--locked",
            "-p",
            "velnor-actions-cli",
            "--bin",
            "velnor-actions",
        ],
    )?;
    let script = format!(
        r#"set -eu; cd "${{GITHUB_WORKSPACE:?}}"; trap 'rm -f "$RUNNER_TEMP/velnor-image-publication-fixture-artifacts.json" "$RUNNER_TEMP/velnor-image-publication-fixture-path.txt"' EXIT; {build} > "$RUNNER_TEMP/velnor-image-publication-fixture-artifacts.json"; VELNOR_INTERNAL_OP=repo-policy-v1 VELNOR_REPO_POLICY_ACTION=artifact-executable VELNOR_REPO_POLICY_ROOT="$GITHUB_WORKSPACE" VELNOR_REPO_POLICY_MESSAGES="$RUNNER_TEMP/velnor-image-publication-fixture-artifacts.json" VELNOR_REPO_POLICY_MANIFEST=crates/velnor-actions-cli/Cargo.toml VELNOR_REPO_POLICY_TARGET=image-publication-fixture VELNOR_REPO_POLICY_ARTIFACT_KIND=example {resolve} > "$RUNNER_TEMP/velnor-image-publication-fixture-path.txt"; IFS= read -r fixture < "$RUNNER_TEMP/velnor-image-publication-fixture-path.txt"; case "$fixture" in /*) test -x "$fixture"; printf 'VELNOR_IMAGE_PUBLICATION_FIXTURE=%s\n' "$fixture" >> "$GITHUB_ENV" ;; *) printf '%s\n' 'Cargo did not select an absolute image publication fixture' >&2; exit 1 ;; esac"#
    );
    let step = velnor_actions_workflow_renderer::shell_step(
        "Prepare image publication fixture",
        vec![
            "bash".to_owned(),
            "-c".to_owned(),
            script,
        ],
        env,
    )?;
    Ok(vec![step])
}

fn mbx_image_fixture_argv(
    catalog: &ToolCatalog,
    cargo_args: &[&str],
) -> Result<String, OrchestratorError> {
    let argv = crate::vectors::exec_argv(
        vec![PinnedTool::Rust],
        "mbx",
        cargo_args,
        catalog,
    )?;
    velnor_actions_workflow_renderer::join_argv_for_run(&argv).map_err(OrchestratorError::from)
}

pub(super) fn obligation_steps(
    input: &CrateJobInputs<'_>,
    model: &CrateJob,
    use_opentofu: bool,
) -> Result<Vec<Step>, OrchestratorError> {
    let sequential = |obligation: &CrateObligation| {
        velnor_actions_rust::runs_before_parallel_checks(&obligation.kind)
    };
    let concurrent = !use_opentofu
        && model
            .obligations
            .iter()
            .filter(|obligation| !sequential(obligation))
            .count()
            >= 2;
    let members: BTreeSet<&str> = model
        .obligations
        .iter()
        .filter(|obligation| concurrent && !sequential(obligation))
        .map(|obligation| obligation.task_id.as_str())
        .collect();
    let target_root =
        crate::source_prep::workspace_root_for_manifest(&model.manifest, input.fetch_roots);
    let seed = seed_step(input, model, concurrent, target_root)?;
    let mut steps = Vec::with_capacity(model.obligations.len() + usize::from(seed.is_some()));
    let mut seed = seed;
    for (index, obligation) in model.obligations.iter().enumerate() {
        let parallel = members.contains(obligation.task_id.as_str());
        if !parallel && steps.iter().any(|step: &Step| step.parallel) {
            return Err(internal("crate_parallel_group_split"));
        }
        if parallel
            && obligation
                .gated_by
                .iter()
                .any(|gate| members.contains(gate.as_str()))
        {
            return Err(internal("crate_parallel_member_gate"));
        }
        if parallel && let Some(seed) = seed.take() {
            steps.push(seed);
        }
        let lane = parallel
            .then(|| velnor_actions_rust::concurrent_target_lane(&obligation.kind))
            .flatten()
            .map(|lane| lane_target_dir(target_root, lane));
        let downstream: Vec<String> = model.obligations[index + 1..]
            .iter()
            .map(|later| later.task_id.clone())
            .collect();
        let cap = (index == 0 && use_opentofu).then_some(input.max_parallel_jobs);
        let mut step = crate::matrix_step::obligation_step(
            obligation,
            input.catalog,
            &downstream,
            cap,
            input.helper_version,
            lane.as_deref(),
        )?;
        step.parallel = parallel;
        if let Some(options) = input.crates.get(&model.package_name) {
            cap_cargo_jobs(&mut step, options.build_jobs);
        }
        steps.push(step);
    }
    Ok(steps)
}

fn lane_target_dir(workspace_root: &str, lane: &str) -> String {
    if workspace_root.is_empty() {
        format!("target/{lane}")
    } else {
        format!("{workspace_root}/target/{lane}")
    }
}

fn seed_step(
    input: &CrateJobInputs<'_>,
    model: &CrateJob,
    concurrent: bool,
    workspace_root: &str,
) -> Result<Option<Step>, OrchestratorError> {
    if !input.seed_parallel_targets
        || !concurrent
        || !model
            .obligations
            .iter()
            .any(|task| velnor_actions_rust::runs_before_parallel_checks(&task.kind))
    {
        return Ok(None);
    }
    let lanes: BTreeSet<&str> = model
        .obligations
        .iter()
        .filter_map(|task| velnor_actions_rust::concurrent_target_lane(&task.kind))
        .collect();
    if lanes.is_empty() {
        return Ok(None);
    }
    let target_root = lane_target_dir(workspace_root, "");
    Ok(Some(
        velnor_actions_workflow_renderer::steps::seed_parallel_targets_step(
            target_root.trim_end_matches('/'),
            &lanes.into_iter().collect::<Vec<_>>(),
        )?,
    ))
}
