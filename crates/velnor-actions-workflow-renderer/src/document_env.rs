//! Workflow/job environment factoring.

use std::collections::BTreeMap;

/// Build the workflow-wide environment from generated repository policy.
///
/// Credential scrubbing always applies globally. Mise isolation becomes
/// workflow-wide only when no repository-declared verification task needs
/// to load its project config; those tasks keep the job-scoped exception.
pub(super) fn workflow_env(verification_tasks_present: bool) -> BTreeMap<String, String> {
    let mut env = crate::toolchain_env::credential_scrub();
    if !verification_tasks_present {
        env.extend(
            crate::toolchain_env::MISE_STATIC_ENV
                .into_iter()
                .map(|(key, value)| (key.to_owned(), value.to_owned())),
        );
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
        let isolated = workflow_env(false);
        let verification = workflow_env(true);

        assert_eq!(
            isolated.get("MISE_NO_CONFIG").map(String::as_str),
            Some("1")
        );
        assert!(!verification.contains_key("MISE_NO_CONFIG"));
        assert_eq!(isolated.get("GH_TOKEN").map(String::as_str), Some(""));
        assert_eq!(verification.get("GH_TOKEN").map(String::as_str), Some(""));
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
