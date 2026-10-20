//! Promote consumed `Compute cache keys` outputs to Plan job outputs.
//!
//! Consumers reference `needs.plan.outputs.<name>` for runtime cache keys;
//! the Plan job promotes the runtime trust plus exactly the consumed key
//! names from its typed key step, so no unconsumed key enters the per-job
//! output budget.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::workflow::cache_family::{
    PLAN_CACHE_OUTPUTS, PLAN_CACHE_OUTPUTS_RESERVE_UTF16_BYTES, PLAN_CACHE_TRUST_OUTPUT,
    cache_output_reserve_utf16_bytes, is_plan_cache_output,
};
use velnor_actions_contract::{Job, Step, StepId, StepKind};

use crate::RenderError;

const NEEDS_PLAN: &str = "needs.plan.outputs.";

/// `(job output, key-step output)` pairs consumed anywhere in `jobs`.
/// # Errors
///
/// Returns [`RenderError`] when a consumed cache output has no Plan key step.
pub(crate) fn consumed(jobs: &BTreeMap<String, Job>) -> Result<Vec<(String, String)>, RenderError> {
    let mut names = BTreeSet::new();
    for step in jobs.values().flat_map(|job| &job.steps) {
        for text in step_texts(step) {
            let mut rest = text;
            while let Some(at) = rest.find(NEEDS_PLAN) {
                rest = &rest[at + NEEDS_PLAN.len()..];
                let end = rest
                    .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                    .unwrap_or(rest.len());
                if is_plan_cache_output(&rest[..end]) {
                    names.insert(rest[..end].to_owned());
                }
            }
        }
    }
    let has_key_step = jobs.get(crate::render::PLAN_JOB_ID).is_some_and(|plan| {
        plan.steps
            .iter()
            .any(|step| step.id == Some(StepId::CacheKeys))
    });
    if has_key_step {
        // Trust is always written and always published for health reporting.
        names.insert(PLAN_CACHE_TRUST_OUTPUT.to_owned());
    } else if names.is_empty() {
        return Ok(Vec::new());
    } else {
        return Err(RenderError::InvalidWorkflow(
            "plan_cache_output_without_key_step".to_owned(),
        ));
    }
    let tofu = names
        .iter()
        .filter(|name| !PLAN_CACHE_OUTPUTS.iter().any(|(output, _)| output == name))
        .count();
    if cache_output_reserve_utf16_bytes(tofu) > PLAN_CACHE_OUTPUTS_RESERVE_UTF16_BYTES {
        return Err(RenderError::InvalidWorkflow(format!(
            "plan_cache_outputs_exceed_reserve:{tofu}"
        )));
    }
    Ok(names
        .into_iter()
        .map(|name| {
            let step_output = PLAN_CACHE_OUTPUTS
                .iter()
                .find(|(output, _)| *output == name)
                .map_or_else(|| name.clone(), |(_, step)| (*step).to_owned());
            (name, step_output)
        })
        .collect())
}

/// Every expression-bearing string of one step.
fn step_texts(step: &Step) -> Vec<&str> {
    let mut texts: Vec<&str> = step.condition.iter().map(String::as_str).collect();
    let (values, env) = match &step.kind {
        StepKind::Action { with, env, .. } => (with.values().collect::<Vec<_>>(), env),
        StepKind::Shell { run, env } => (run.iter().collect(), env),
        StepKind::TaskExecution { argv, env, .. } => (argv.iter().collect(), env),
        StepKind::Internal { env, .. } => (Vec::new(), env),
    };
    texts.extend(values.into_iter().map(String::as_str));
    texts.extend(env.values().map(String::as_str));
    texts
}

/// Job-output entries for the consumed cache outputs.
pub(crate) fn output_entries(cache: &[(String, String)]) -> Vec<(String, crate::yaml::Yaml)> {
    cache
        .iter()
        .map(|(name, step_output)| {
            (
                name.clone(),
                crate::yaml::Yaml::str(format!(
                    "${{{{ steps.{}.outputs.{step_output} }}}}",
                    StepId::CacheKeys.as_str()
                )),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use velnor_actions_contract::{Job, JobTimeout, Step, StepId, StepKind};

    fn job(steps: Vec<Step>) -> Job {
        Job {
            services: Vec::new(),
            display_name: "Job".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::PLAN,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            steps,
            cache_mode: None,
        }
    }

    fn step(id: Option<StepId>, key: &str) -> Step {
        Step {
            parallel: false,
            name: "Step".to_owned(),
            id,
            role: None,
            condition: None,
            kind: StepKind::Action {
                uses: "actions/cache/restore@x".to_owned(),
                with: BTreeMap::from([("key".to_owned(), key.to_owned())]),
                env: BTreeMap::new(),
            },
        }
    }

    #[test]
    fn plan_promotes_trust_and_exactly_the_consumed_keys() {
        let jobs = BTreeMap::from([
            (
                "plan".to_owned(),
                job(vec![step(Some(StepId::CacheKeys), "k")]),
            ),
            (
                "rust-a".to_owned(),
                job(vec![step(None, "${{needs.plan.outputs.sources_key}}")]),
            ),
        ]);
        let outputs = super::consumed(&jobs).expect("consumed");
        assert_eq!(
            outputs,
            [
                ("cache_trust".to_owned(), "trust".to_owned()),
                ("sources_key".to_owned(), "sources_key".to_owned()),
            ]
        );
        let entries = super::output_entries(&outputs);
        assert!(matches!(&entries[0].1, crate::yaml::Yaml::Str(value)
            if value == "${{ steps.cache-keys.outputs.trust }}"));
    }

    #[test]
    fn consumed_cache_outputs_require_the_plan_key_step() {
        let jobs = BTreeMap::from([
            ("plan".to_owned(), job(Vec::new())),
            (
                "rust-a".to_owned(),
                job(vec![step(None, "${{needs.plan.outputs.sources_key}}")]),
            ),
        ]);
        let err = super::consumed(&jobs).expect_err("no key step");
        assert!(
            err.to_string()
                .contains("plan_cache_output_without_key_step")
        );
        let plain = BTreeMap::from([("plan".to_owned(), job(Vec::new()))]);
        assert_eq!(super::consumed(&plain).expect("none"), Vec::new());
    }
}
