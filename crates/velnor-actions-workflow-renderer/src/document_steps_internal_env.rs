//! Internal-step environment projection for the workflow document renderer.

use std::collections::BTreeMap;

use crate::{
    RenderError, commands,
    render::RenderContext,
    steps::{self, INTERNAL_OP_ENV, REQUEST_FILE_ENV},
    yaml::Yaml,
};

/// Borrowed invocation context for rendering an internal step.
#[derive(Clone, Copy)]
pub(super) struct InternalStepContext<'a> {
    pub(super) job_id: &'a str,
    pub(super) job_env: &'a BTreeMap<String, String>,
    pub(super) step_env: &'a BTreeMap<String, String>,
    pub(super) composite: bool,
}

/// Env for one internal step: op plus request file, authorized reads carry auth.
///
/// The fetch op takes no request file; it reads the plan from the run directory and
/// authenticates `gh` with the job token plus the repository slug (fixed
/// literals, never caller input). Merge-family steps in the final job additionally
/// carry the finalized `needs` conclusions channel and rendered expected inventory.
pub(super) fn internal_env(
    op: &str,
    target: &str,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
    step: InternalStepContext<'_>,
) -> Result<Yaml, RenderError> {
    if op == steps::FETCH_OPERATION {
        return fetch_internal_env(op, step.step_env);
    }
    if let Some(env) = crate::document_evidence::internal_env(op, step.step_env)? {
        return Ok(env);
    }
    if is_private_source_operation(op) {
        return private_source_env(op, step.step_env);
    }
    if steps::is_qualification_cache_operation(op) {
        return qualification_cache_env(op, step.step_env, step.composite);
    }
    request_file_env(op, target, ctx, needs_envs, step)
}

fn fetch_internal_env(op: &str, step_env: &BTreeMap<String, String>) -> Result<Yaml, RenderError> {
    commands::validate_env(step_env)?;
    if !step_env.is_empty() {
        return Err(RenderError::InvalidWorkflow(
            "fetch_internal_env_not_supported".to_owned(),
        ));
    }
    Ok(Yaml::Map(vec![
        ("GH_REPO".to_owned(), Yaml::str("${{ github.repository }}")),
        ("GH_TOKEN".to_owned(), Yaml::str("${{ github.token }}")),
        (INTERNAL_OP_ENV.to_owned(), Yaml::str(op.to_owned())),
    ]))
}

fn is_private_source_operation(op: &str) -> bool {
    op == velnor_actions_contract::PRIVATE_FETCH_OPERATION
        || op == velnor_actions_contract::PRIVATE_REVOKE_OPERATION
        || op == velnor_actions_contract::GRADLE_RUN_OPERATION
        || op == velnor_actions_contract::BUN_REGISTRY_INSTALL_OPERATION
        || op == velnor_actions_contract::BUN_ASSETS_EXPORT_OPERATION
        || op == velnor_actions_contract::BUN_ASSETS_PREPARE_OPERATION
        || op == velnor_actions_contract::BUN_ASSETS_IMPORT_OPERATION
        || op == velnor_actions_contract::config::playwright_lock::PLAYWRIGHT_INSTALL_OPERATION
}

fn private_source_env(op: &str, step_env: &BTreeMap<String, String>) -> Result<Yaml, RenderError> {
    commands::validate_env(step_env)?;
    // Private-source operations take no request file: their whole input
    // is the fixed step env (token binding plus the literal allowlist).
    let mut env = vec![(INTERNAL_OP_ENV.to_owned(), Yaml::str(op.to_owned()))];
    env.extend(
        step_env
            .iter()
            .map(|(key, value)| (key.clone(), Yaml::str(value.clone()))),
    );
    Ok(Yaml::Map(env))
}

fn qualification_cache_env(
    op: &str,
    step_env: &BTreeMap<String, String>,
    composite: bool,
) -> Result<Yaml, RenderError> {
    if composite {
        commands::validate_composite_qualification_cache_env(step_env)?;
    } else {
        commands::validate_qualification_cache_env(step_env)?;
    }
    super::cache_env::validate_qualification_cache_env(op, step_env)?;
    let mut env = vec![(INTERNAL_OP_ENV.to_owned(), Yaml::str(op.to_owned()))];
    env.extend(
        step_env
            .iter()
            .map(|(key, value)| (key.clone(), Yaml::str(value.clone()))),
    );
    Ok(Yaml::Map(env))
}

fn request_file_env(
    op: &str,
    target: &str,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
    step: InternalStepContext<'_>,
) -> Result<Yaml, RenderError> {
    commands::validate_env(step.step_env)?;
    let request = format!("{}/{target}-request.json", ctx.request_dir);
    let mut env = Vec::new();
    if (step.job_id == crate::render::PLAN_JOB_ID
        && matches!(
            op,
            steps::RESOLVE_QUALIFICATION_OPERATION | steps::PLAN_OPERATION
        )
        && target == steps::PLAN_OPERATION)
        || (step.job_id == crate::render::FINAL_JOB_ID
            && op == steps::MERGE_OPERATION
            && target == steps::MERGE_OPERATION)
    {
        env.push(("GH_TOKEN".to_owned(), Yaml::str("${{ github.token }}")));
    }
    env.push((INTERNAL_OP_ENV.to_owned(), Yaml::str(op.to_owned())));
    for (key, value) in needs_envs {
        env.push((key.clone(), Yaml::str(value.clone())));
    }
    env.push((REQUEST_FILE_ENV.to_owned(), Yaml::str(request)));
    if op == steps::PLAN_OPERATION && target == steps::PLAN_OPERATION {
        for (key, value) in &ctx.plan_consumer_env {
            if step.job_env.get(key).map(String::as_str) != Some(value.as_str()) {
                env.push((key.clone(), Yaml::str(value.clone())));
            }
        }
    }
    let mut seen: std::collections::BTreeSet<String> =
        env.iter().map(|(key, _)| key.clone()).collect();
    for (key, value) in step.step_env {
        if !seen.insert(key.clone()) {
            return Err(RenderError::InvalidWorkflow(format!(
                "duplicate_internal_env:{key}"
            )));
        }
        env.push((key.clone(), Yaml::str(value.clone())));
    }
    Ok(Yaml::Map(env))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::render::{FINAL_JOB_ID, PLAN_JOB_ID, RenderContext};
    use velnor_actions_contract::PullRequestCachePolicy;

    fn context() -> RenderContext {
        RenderContext {
            tool_pins: Vec::new(),
            generator_version: "0.1.0".to_owned(),
            report_helper_version: "0.1.0".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            scale_set_selector: None,
            scale_set_image_source_sha: None,
            staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
            verified_cache_helpers: BTreeMap::new(),
            require_verified_cache_helper: false,
            request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
            checkout_uses: "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1".to_owned(),
            validator_commands: Vec::new(),
            candidate: None,
            preseed: false,
            workflow_tasks: Vec::new(),
            pull_request_cache_policy: PullRequestCachePolicy::ReadOnly,
            mise_quiet: true,
            plan_consumer_env: BTreeMap::new(),
        }
    }

    fn rendered_env(job: &str, operation: &str, target: &str) -> String {
        let job_env = BTreeMap::new();
        let step_env = BTreeMap::new();
        let rendered = internal_env(
            operation,
            target,
            &context(),
            &[],
            InternalStepContext {
                job_id: job,
                job_env: &job_env,
                step_env: &step_env,
                composite: false,
            },
        )
        .expect("typed internal env");
        crate::yaml::render_yaml(&rendered)
    }

    fn rendered_step(job: &str, operation: &str) -> String {
        let step = steps::internal_step("Typed internal operation", operation)
            .expect("fixed internal operation");
        let job_env = BTreeMap::new();
        let rendered = crate::document_steps::internal_step_to_yaml(
            job,
            &step,
            &context(),
            &[],
            false,
            &crate::document_lanes::JobStepContext {
                job_env: &job_env,
                runs_on: None,
                actions_read: true,
            },
        )
        .expect("typed internal step renders");
        crate::yaml::render_yaml(&rendered)
    }

    #[test]
    fn consumer_plan_and_required_merge_receive_step_scoped_read_token() {
        let plan = rendered_step(PLAN_JOB_ID, steps::PLAN_OPERATION);
        assert!(plan.contains("GH_TOKEN: ${{ github.token }}"), "{plan}");
        let merge = rendered_step(FINAL_JOB_ID, steps::MERGE_OPERATION);
        assert!(merge.contains("GH_TOKEN: ${{ github.token }}"), "{merge}");
    }

    #[test]
    fn wrong_job_or_operation_does_not_receive_token() {
        for (job, operation, target) in [
            ("generic", steps::PLAN_OPERATION, steps::PLAN_OPERATION),
            (FINAL_JOB_ID, steps::PLAN_OPERATION, steps::PLAN_OPERATION),
            (PLAN_JOB_ID, steps::MERGE_OPERATION, steps::MERGE_OPERATION),
            (PLAN_JOB_ID, steps::PLAN_OPERATION, "other-v1"),
            ("generic", steps::MERGE_OPERATION, steps::MERGE_OPERATION),
            (
                "generic",
                steps::RESOLVE_QUALIFICATION_OPERATION,
                steps::PLAN_OPERATION,
            ),
        ] {
            let rendered = rendered_env(job, operation, target);
            assert!(
                !rendered.contains("GH_TOKEN"),
                "{job}/{operation}/{target}: {rendered}"
            );
        }
        let resolver = rendered_env(
            PLAN_JOB_ID,
            steps::RESOLVE_QUALIFICATION_OPERATION,
            steps::PLAN_OPERATION,
        );
        assert!(resolver.contains("GH_TOKEN: ${{ github.token }}"));
    }

    #[test]
    fn generic_internal_step_env_cannot_inject_github_expression() {
        let step_env = BTreeMap::from([("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned())]);
        let job_env = BTreeMap::new();
        let error = internal_env(
            steps::PLAN_OPERATION,
            steps::PLAN_OPERATION,
            &context(),
            &[],
            InternalStepContext {
                job_id: "generic",
                job_env: &job_env,
                step_env: &step_env,
                composite: false,
            },
        )
        .expect_err("generic env remains under unchanged expression validation");
        assert!(error.to_string().contains("bad_env_expression:GH_TOKEN"));
    }
}
