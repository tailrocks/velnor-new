//! Per-job hosted runners qualified inside the repository workflow.

use velnor_actions_contract::RunsOn;

use crate::RenderError;

/// Native Apple Silicon label qualified by the repository's release jobs.
pub(crate) const MACOS_RUNS_ON: &str = "macos-15";

/// Require one job's label: default, owned task, typed scale-set, or macOS ARM.
///
/// The policy label gate calls this per job because check-runner and
/// verification-task jobs legitimately carry non-default labels, so no
/// whole-workflow loop can admit them. Per-job cache targets stay with
/// `runs_on::target_for_runner`, which additionally fails closed on
/// unqualified scale-set labels.
pub(crate) fn check_job_label(
    id: &str,
    runs_on: &str,
    default_label: &str,
    task_label: Option<&str>,
) -> Result<(), RenderError> {
    let scale_set = RunsOn::parse(runs_on).is_ok_and(|selector| selector.is_scale_set());
    if runs_on != default_label
        && task_label != Some(runs_on)
        && !scale_set
        && runs_on != MACOS_RUNS_ON
    {
        return Err(RenderError::InvalidWorkflow(format!("label_mismatch:{id}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualified_macos_runner_is_allowed() {
        assert!(check_job_label("macos", MACOS_RUNS_ON, "ubuntu-26.04", None).is_ok());
        assert!(check_job_label("linux", "ubuntu-26.04", "ubuntu-26.04", None).is_ok());
        assert!(
            check_job_label(
                "scale-set",
                "scale-set:velnor+ubuntu-26.04-scale-set",
                "ubuntu-26.04",
                None
            )
            .is_ok()
        );
        assert!(
            check_job_label(
                "owned",
                "ubuntu-24.04",
                "ubuntu-26.04",
                Some("ubuntu-24.04")
            )
            .is_ok()
        );
    }

    #[test]
    fn arbitrary_hosted_runner_does_not_bypass_default_label_gate() {
        for label in ["macos-latest", "macos-14", "windows-2025", "ubuntu-24.04"] {
            let error = check_job_label("job", label, "ubuntu-26.04", None)
                .expect_err("unqualified hosted runner fails");
            assert!(error.to_string().contains("label_mismatch:job"), "{error}");
        }
    }
}
