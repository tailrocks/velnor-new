//! Job permission, placement and dependency validation.
use super::{
    BTreeSet, ContractError, Job, PermissionLevel, Permissions, is_pinned_label,
    is_safe_display_name, is_valid_environment,
};
impl Job {
    /// Validate runner metadata before rendering placement.
    fn validate_runner(&self, id: &str) -> Result<(), ContractError> {
        if let Some(runner) = &self.check_runner {
            if !id.starts_with("check-") {
                return Err(ContractError::identity(
                    "job.check_runner",
                    "check_runner_requires_check_job",
                ));
            }
            runner.validate("workflow", &format!("jobs.{id}.check_runner"))?;
            if runner.executor == crate::config::CheckExecutor::EphemeralSelfHosted
                && self.condition.as_deref()
                    != Some(crate::config::EPHEMERAL_CHECK_ADMISSION_CONDITION)
            {
                return Err(ContractError::identity(
                    "job.condition",
                    "ephemeral_check_requires_admission_condition",
                ));
            }
            let scale_set = crate::config::RunsOn::parse(&self.runs_on)
                .is_ok_and(|selector| selector.is_scale_set());
            if scale_set
                && (runner.platform != crate::config::CheckPlatform::LinuxX64
                    || runner.executor != crate::config::CheckExecutor::Hosted
                    || self.condition.as_deref()
                        != Some(crate::config::EPHEMERAL_CHECK_ADMISSION_CONDITION))
            {
                return Err(ContractError::identity(
                    "job.runs_on",
                    "check_runner_scale_set_mismatch",
                ));
            }
            if !scale_set && runner.label != self.runs_on {
                return Err(ContractError::identity(
                    "job.runs_on",
                    "check_runner_label_mismatch",
                ));
            }
        } else if id.starts_with("check-") {
            return Err(ContractError::identity(
                "job.check_runner",
                "missing_check_runner",
            ));
        } else if !is_pinned_label(&self.runs_on) {
            return Err(ContractError::identity(
                "job.runs_on",
                format!("unpinned_label:{id}"),
            ));
        }
        Ok(())
    }
    /// Validate one job: labels, refs, effective permissions, steps.
    pub(super) fn validate(
        &self,
        id: &str,
        ids: &BTreeSet<&str>,
        workflow: &Permissions,
        pr_triggered: bool,
    ) -> Result<(), ContractError> {
        if self.display_name.trim().is_empty() {
            return Err(ContractError::identity("job.display_name", "empty_name"));
        }
        if !is_safe_display_name(&self.display_name) {
            return Err(ContractError::identity(
                "job.display_name",
                format!("bad_display_name:{id}"),
            ));
        }
        self.validate_runner(id)?;
        self.timeout_minutes.validate()?;
        for need in &self.needs {
            if !ids.contains(need.as_str()) {
                return Err(ContractError::identity(
                    "job.needs",
                    format!("unknown_job:{need}"),
                ));
            }
        }
        if self
            .permissions
            .as_ref()
            .is_some_and(Permissions::is_write_all)
        {
            return Err(ContractError::identity(
                "job.permissions",
                format!("write_all:{id}"),
            ));
        }
        if let Some(environment) = &self.environment
            && !is_valid_environment(environment)
        {
            return Err(ContractError::identity(
                "job.environment",
                format!("bad_environment:{id}"),
            ));
        }
        let effective = self.permissions.as_ref().unwrap_or(workflow);
        if matches!(effective.id_token, PermissionLevel::Write) && self.environment.is_none() {
            return Err(ContractError::identity(
                "job.environment",
                format!("id_token_write_needs_environment:{id}"),
            ));
        }
        if pr_triggered && matches!(effective.contents, PermissionLevel::Write) {
            return Err(ContractError::identity(
                "job.permissions",
                format!("contents_write_on_pr:{id}"),
            ));
        }
        if self.steps.is_empty() {
            return Err(ContractError::identity(
                "job.steps",
                format!("empty_steps:{id}"),
            ));
        }
        super::super::step_identity::validate_step_sequence(&self.steps, id)?;
        Ok(())
    }
}

#[cfg(test)]
mod admission_tests {
    use super::*;
    use crate::config::{
        CheckExecutor, CheckPlatform, CheckRunner, EPHEMERAL_CHECK_ADMISSION_CONDITION,
    };

    fn ephemeral_job(condition: Option<&str>) -> Job {
        Job {
            display_name: "Check / native".to_owned(),
            runs_on: "native-scale-set".to_owned(),
            check_runner: Some(CheckRunner {
                label: "native-scale-set".to_owned(),
                platform: CheckPlatform::MacosArm64,
                executor: CheckExecutor::EphemeralSelfHosted,
                container: None,
            }),
            timeout_minutes: super::super::JobTimeout::CRATE,
            needs: Vec::new(),
            condition: condition.map(str::to_owned),
            permissions: None,
            environment: None,
            steps: Vec::new(),
        }
    }

    #[test]
    fn external_runner_admission_is_mandatory_even_for_public_typed_ir() {
        for condition in [
            None,
            Some("true"),
            Some("always()"),
            Some("success()"),
            Some("github.event_name == 'pull_request'"),
        ] {
            let error = ephemeral_job(condition)
                .validate_runner("check-native")
                .expect_err("external execution requires exact admission");
            assert!(
                error
                    .to_string()
                    .contains("ephemeral_check_requires_admission_condition")
            );
        }
        ephemeral_job(Some(EPHEMERAL_CHECK_ADMISSION_CONDITION))
            .validate_runner("check-native")
            .expect("canonical admission accepted");
    }
}
