// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{CrateJobInputs, build_jobs::cap_cargo_jobs};
use crate::{OrchestratorError, internal::internal};
use std::collections::{BTreeMap, BTreeSet};
use velnor_actions_contract::{CrateJob, CrateObligation, Step, StepKind, WorkflowPolicy};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

#[cfg(test)]
#[path = "crate_jobs_steps_tuf_tests.rs"]
mod tuf_tests;

pub(super) fn fixture_checkout(needs_fixture: bool) -> Result<Step, OrchestratorError> {
    let mut checkout = crate::workflow::wire_w1::checkout_step()?;
    if needs_fixture {
        let StepKind::Action { with, .. } = &mut checkout.kind else {
            return Err(OrchestratorError::Contract {
                problem: "fixture checkout is not an action".to_owned(),
            });
        };
        with.insert(
            "filter".to_owned(),
            velnor_actions_contract::workflow::step_identity::FIXTURE_CHECKOUT_BLOB_FILTER
                .to_owned(),
        );
    }
    Ok(checkout)
}

pub(super) fn gh_fixture_step(
    policy: WorkflowPolicy,
    model: &CrateJob,
) -> Result<Option<Step>, OrchestratorError> {
    if policy == WorkflowPolicy::VelnorRepositoryV1
        && matches!(
            model.package_name.as_str(),
            "velnor-actions-cli" | "velnor-actions-orchestrator"
        )
        && model.obligations.iter().any(|obligation| {
            obligation.kind == "test" || obligation.run.iter().any(|arg| arg == "nextest")
        })
    {
        let mut argv = vec![
            "/usr/bin/python3".to_owned(),
            "-I".to_owned(),
            "scripts/prepare-gh-fixture-tools.py".to_owned(),
        ];
        if model.package_name == "velnor-actions-cli"
            && model.obligations.iter().any(image_publication_test)
        {
            argv.push("--with-image-tuf".to_owned());
        }
        return Ok(Some(velnor_actions_workflow_renderer::shell_step(
            "Stage isolated GitHub CLI test tools",
            argv,
            BTreeMap::new(),
        )?));
    }
    Ok(None)
}

fn image_publication_test(obligation: &CrateObligation) -> bool {
    if obligation.kind == "nextest" {
        return true;
    }
    if obligation.kind != "test" {
        return false;
    }
    obligation.run.iter().any(|arg| {
        matches!(arg.as_str(), "--tests" | "--test" | "--all-targets") || arg.starts_with("--test=")
    }) || !obligation.run.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--lib" | "--bins" | "--examples" | "--benches"
        )
    })
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
    let env = crate::matrix_step::task_step_env(input.catalog, &BTreeMap::new(), true)?;
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
        vec!["bash".to_owned(), "-c".to_owned(), script],
        env,
    )?;
    Ok(vec![step])
}

fn mbx_image_fixture_argv(
    catalog: &ToolCatalog,
    cargo_args: &[&str],
) -> Result<String, OrchestratorError> {
    let argv =
        crate::product_release_pins::exec_argv(&[PinnedTool::Rust], "mbx", cargo_args, catalog)?;
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
        .filter_map(|task| velnor_actions_rust::check_target_lane(&task.kind))
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
