//! Workflow/job environment factoring.

use std::collections::BTreeMap;

use velnor_actions_contract::Job;

use crate::render::{PLAN_ATTEMPT_ENV, PLAN_JOB_ID};

/// `VELNOR_RUN_KEY_ATTEMPT` value for one job of a workflow with a plan job.
///
/// The plan job binds its own attempt; jobs that need it bind the attempt
/// that produced the plan, so a "re-run failed jobs" attempt keeps every
/// run-scoped identity (plan artifact, run directory, report run key,
/// crate artifact names) on the plan its first attempt produced; a missing
/// plan output falls back to the current attempt so diagnostics still
/// land. Other jobs keep their own attempt. Workflows without a plan job
/// get nothing.
pub(super) fn plan_attempt_value(
    id: &str,
    job: &Job,
    jobs: &BTreeMap<String, Job>,
) -> Option<&'static str> {
    if !jobs.contains_key(PLAN_JOB_ID) {
        return None;
    }
    if id != PLAN_JOB_ID && job.needs.iter().any(|need| need == PLAN_JOB_ID) {
        Some("${{ needs.plan.outputs.plan_attempt || github.run_attempt }}")
    } else {
        Some("${{ github.run_attempt }}")
    }
}

/// Add the native MBX policy and the plan-attempt binding to a job env.
pub(super) fn extend_job_env(
    job_env: &mut BTreeMap<String, String>,
    native_mbx: bool,
    plan_attempt: Option<&str>,
) {
    if native_mbx {
        crate::mbx_job_env::extend_mbx_job_env(job_env);
    }
    if let Some(value) = plan_attempt {
        job_env.insert(PLAN_ATTEMPT_ENV.to_owned(), value.to_owned());
    }
}

/// Build the workflow-wide environment from generated repository policy.
///
/// Credential scrubbing always applies globally. Mise isolation becomes
/// workflow-wide only when no repository-declared verification task needs
/// to load its project config; those tasks keep the job-scoped exception.
/// `mise_quiet = false` (consumer `workflow.mise_quiet`) omits `MISE_QUIET`
/// so the Mise wrapper-dispatch trace stays visible; the default silences it.
pub(super) fn workflow_env(
    verification_tasks_present: bool,
    mise_quiet: bool,
) -> BTreeMap<String, String> {
    let mut env = crate::toolchain_env::credential_scrub();
    if !verification_tasks_present {
        env.extend(
            crate::toolchain_env::MISE_STATIC_ENV
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value.to_owned())),
        );
        if mise_quiet {
            env.insert("MISE_QUIET".to_owned(), "1".to_owned());
        }
    }
    env
}

/// Remove job values already inherited unchanged from workflow scope.
pub(super) fn exclude_inherited(
    job: &BTreeMap<String, String>,
    workflow: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    job.iter()
        .filter(|(key, value)| workflow.get(*key) != Some(*value))
        .map(|(key, value)| ((*key).clone(), (*value).clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{exclude_inherited, workflow_env};
    use std::collections::BTreeMap;

    #[test]
    fn workflow_policy_keeps_verification_mise_config_available() {
        let isolated = workflow_env(false, true);
        let verification = workflow_env(true, true);

        assert_eq!(
            isolated.get("MISE_NO_CONFIG").map(String::as_str),
            Some("1")
        );
        assert_eq!(isolated.get("MISE_QUIET").map(String::as_str), Some("1"));
        assert!(!verification.contains_key("MISE_NO_CONFIG"));
        assert!(!verification.contains_key("MISE_QUIET"));
        assert_eq!(isolated.get("GH_TOKEN").map(String::as_str), Some(""));
        assert_eq!(verification.get("GH_TOKEN").map(String::as_str), Some(""));
    }

    #[test]
    fn mise_quiet_opt_out_omits_only_the_quiet_key() {
        let silenced = workflow_env(false, true);
        let loud = workflow_env(false, false);
        let verification = workflow_env(true, false);

        assert!(!loud.contains_key("MISE_QUIET"));
        let mut expected = silenced;
        expected.remove("MISE_QUIET");
        assert_eq!(loud, expected, "silencing changes only MISE_QUIET");
        assert!(!verification.contains_key("MISE_QUIET"));
        assert_eq!(loud.get("MISE_NO_CONFIG").map(String::as_str), Some("1"));
    }

    #[test]
    fn retains_job_overrides_and_job_only_values() {
        let workflow = BTreeMap::from([
            ("shared".to_owned(), String::new()),
            ("override".to_owned(), String::new()),
        ]);
        let job = BTreeMap::from([
            ("shared".to_owned(), String::new()),
            ("override".to_owned(), "specific".to_owned()),
            ("job_only".to_owned(), "value".to_owned()),
        ]);

        assert_eq!(
            exclude_inherited(&job, &workflow),
            BTreeMap::from([
                ("job_only".to_owned(), "value".to_owned()),
                ("override".to_owned(), "specific".to_owned()),
            ])
        );
    }
}
