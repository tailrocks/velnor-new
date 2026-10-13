//! Per-job hosted runners qualified inside the repository workflow.

use velnor_actions_contract::{Job, RunsOn, WorkflowIr, target_for_runner_label};

use crate::RenderError;

/// Native Apple Silicon label qualified by the repository's release jobs.
const MACOS_RUNS_ON: &str = "macos-15";
/// Cache platform for the native Apple Silicon runner.
const MACOS_TARGET: &str = "aarch64-apple-darwin";

/// Require the workflow default runner, its typed scale-set variant, or macOS ARM.
pub(crate) fn validate_job_runners(
    ir: &WorkflowIr,
    default_label: &str,
) -> Result<(), RenderError> {
    for (id, job) in &ir.jobs {
        let scale_set = RunsOn::parse(&job.runs_on).is_ok_and(|selector| selector.is_scale_set());
        if job.runs_on != default_label && !scale_set && job.runs_on != MACOS_RUNS_ON {
            return Err(RenderError::InvalidWorkflow(format!("label_mismatch:{id}")));
        }
    }
    Ok(())
}

/// Resolve the platform used to key a job's installed-tool cache.
pub(crate) fn cache_target_for_job(job: &Job, default_label: &str) -> Option<&'static str> {
    if job.runs_on == MACOS_RUNS_ON {
        return Some(MACOS_TARGET);
    }
    if RunsOn::parse(&job.runs_on).is_ok_and(|selector| selector.is_scale_set()) {
        return target_for_runner_label(default_label);
    }
    target_for_runner_label(&job.runs_on)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use velnor_actions_contract::{JobTimeout, Permissions, Step, StepKind};

    /// Minimal workflow with jobs on the requested literal labels.
    fn workflow(runners: &[(&str, &str)]) -> WorkflowIr {
        let jobs = runners
            .iter()
            .map(|(id, label)| {
                (
                    (*id).to_owned(),
                    Job {
                        display_name: (*id).to_owned(),
                        runs_on: (*label).to_owned(),
                        timeout_minutes: JobTimeout::VALIDATOR,
                        needs: Vec::new(),
                        condition: None,
                        permissions: None,
                        environment: None,
                        steps: vec![Step {
                            name: "Run".to_owned(),
                            condition: None,
                            kind: StepKind::Shell {
                                run: vec!["true".to_owned()],
                                env: BTreeMap::new(),
                            },
                        }],
                    },
                )
            })
            .collect();
        WorkflowIr {
            name: "CI".to_owned(),
            triggers: velnor_actions_contract::Trigger {
                pull_request_types: Vec::new(),
                push_branches: vec!["main".to_owned()],
                merge_group: true,
                workflow_dispatch: None,
                schedule: None,
            },
            permissions: Permissions::default(),
            concurrency: velnor_actions_contract::Concurrency {
                group: "ci".to_owned(),
                cancel_in_progress: "false".to_owned(),
            },
            jobs,
        }
    }

    #[test]
    fn only_qualified_macos_runner_is_allowed_and_gets_arm_cache_key() {
        let ir = workflow(&[("linux", "ubuntu-26.04"), ("macos", MACOS_RUNS_ON)]);
        assert!(validate_job_runners(&ir, "ubuntu-26.04").is_ok());
        assert_eq!(
            cache_target_for_job(&ir.jobs["macos"], "ubuntu-26.04"),
            Some(MACOS_TARGET)
        );
        assert_eq!(
            cache_target_for_job(&ir.jobs["linux"], "ubuntu-26.04"),
            Some("x86_64-unknown-linux-gnu")
        );
    }

    #[test]
    fn arbitrary_hosted_runner_does_not_bypass_default_label_gate() {
        for label in ["macos-latest", "macos-14", "windows-2025"] {
            let ir = workflow(&[("job", label)]);
            let error = validate_job_runners(&ir, "ubuntu-26.04")
                .expect_err("unqualified hosted runner fails");
            assert!(error.to_string().contains("label_mismatch:job"), "{error}");
        }
    }
}
