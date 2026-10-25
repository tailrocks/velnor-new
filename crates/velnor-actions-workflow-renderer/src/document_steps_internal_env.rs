use std::collections::BTreeMap;

use crate::{
    RenderError, commands,
    render::RenderContext,
    steps::{self, INTERNAL_OP_ENV, REQUEST_FILE_ENV},
    yaml::Yaml,
};

pub(super) fn internal_env(
    job_id: &str,
    op: &str,
    target: &str,
    ctx: &RenderContext,
    needs_envs: &[(String, String)],
    job_env: &BTreeMap<String, String>,
    step_env: &BTreeMap<String, String>,
) -> Result<Yaml, RenderError> {
    commands::validate_env(step_env)?;
    if op == steps::FETCH_OPERATION {
        if !step_env.is_empty() {
            return Err(RenderError::InvalidWorkflow(
                "fetch_internal_env_not_supported".to_owned(),
            ));
        }
        return Ok(Yaml::Map(vec![
            ("GH_REPO".to_owned(), Yaml::str("${{ github.repository }}")),
            ("GH_TOKEN".to_owned(), Yaml::str("${{ github.token }}")),
            (INTERNAL_OP_ENV.to_owned(), Yaml::str(op.to_owned())),
        ]));
    }
    if let Some(env) = crate::document_evidence::internal_env(op, step_env)? {
        return Ok(env);
    }
    if op == velnor_actions_contract::PRIVATE_FETCH_OPERATION
        || op == velnor_actions_contract::PRIVATE_REVOKE_OPERATION
        || op == velnor_actions_contract::GRADLE_RUN_OPERATION
        || op == velnor_actions_contract::BUN_REGISTRY_INSTALL_OPERATION
        || op == velnor_actions_contract::BUN_ASSETS_EXPORT_OPERATION
        || op == velnor_actions_contract::BUN_ASSETS_PREPARE_OPERATION
        || op == velnor_actions_contract::BUN_ASSETS_IMPORT_OPERATION
        || op == velnor_actions_contract::config::playwright_lock::PLAYWRIGHT_INSTALL_OPERATION
    {
        let mut env = vec![(INTERNAL_OP_ENV.to_owned(), Yaml::str(op.to_owned()))];
        env.extend(
            step_env
                .iter()
                .map(|(key, value)| (key.clone(), Yaml::str(value.clone()))),
        );
        return Ok(Yaml::Map(env));
    }
    let request = format!("{}/{target}-request.json", ctx.request_dir);
    let mut env = Vec::new();
    if (job_id == crate::render::PLAN_JOB_ID
        && matches!(
            op,
            steps::RESOLVE_QUALIFICATION_OPERATION | steps::PLAN_OPERATION
        )
        && target == steps::PLAN_OPERATION)
        || (job_id == crate::render::FINAL_JOB_ID
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
            if job_env.get(key).map(String::as_str) != Some(value.as_str()) {
                env.push((key.clone(), Yaml::str(value.clone())));
            }
        }
    }
    let mut seen: std::collections::BTreeSet<String> =
        env.iter().map(|(key, _)| key.clone()).collect();
    for (key, value) in step_env {
        if !seen.insert(key.clone()) {
            return Err(RenderError::InvalidWorkflow(format!(
                "duplicate_internal_env:{key}"
            )));
        }
        env.push((key.clone(), Yaml::str(value.clone())));
    }
    Ok(Yaml::Map(env))
}
