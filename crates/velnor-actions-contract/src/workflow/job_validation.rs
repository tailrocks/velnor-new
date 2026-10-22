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
        if let Some(cache_mode) = &self.cache_mode {
            cache_mode.validate("job.cache_mode")?;
        }
        let mut service_ids = BTreeSet::new();
        for service in &self.services {
            service.validate()?;
            if !service_ids.insert(service.id.as_str()) {
                return Err(ContractError::identity(
                    "job.services",
                    format!("duplicate_service:{}", service.id),
                ));
            }
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
        super::super::dependency_seed::validate_job(self, id, effective)?;
        if matches!(effective.id_token, PermissionLevel::Write) && self.environment.is_none() {
            return Err(ContractError::identity(
                "job.environment",
                format!("id_token_write_needs_environment:{id}"),
            ));
        }
        if matches!(effective.packages, PermissionLevel::Write) {
            return Err(ContractError::identity(
                "job.permissions",
                format!("packages_write_outside_publication:{id}"),
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
        super::super::step_identity::validate_step_sequence(&self.steps, id)
    }
}

#[cfg(test)]
mod admission_tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use crate::config::{
        CheckExecutor, CheckPlatform, CheckRunner, EPHEMERAL_CHECK_ADMISSION_CONDITION,
    };

    fn ephemeral_job(condition: Option<&str>) -> Job {
        Job {
            services: Vec::new(),
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
            cache_mode: None,
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

    fn verifier_seed_job(verifier_uses: &str) -> Job {
        use crate::workflow::{RUST_VERIFIER_USES, Step, StepKind, StepRole};

        assert!(RUST_VERIFIER_USES.contains(&verifier_uses));
        let mut steps = vec![Step {
            name: "Verify Rust channel manifest".to_owned(),
            id: None,
            role: Some(StepRole::VerifyRustManifest),
            condition: None,
            parallel: false,
            kind: StepKind::Action {
                uses: verifier_uses.to_owned(),
                with: BTreeMap::new(),
                env: BTreeMap::new(),
            },
        }];
        steps.extend((0..3).map(|index| Step {
            name: format!("Build seed {index}"),
            id: None,
            role: Some(StepRole::MbxSeedBuild),
            condition: None,
            parallel: false,
            kind: StepKind::Shell {
                run: vec!["true".to_owned()],
                env: BTreeMap::new(),
            },
        }));
        steps.push(Step {
            name: "Mark seed producer".to_owned(),
            id: None,
            role: Some(StepRole::MbxProducerMark),
            condition: None,
            parallel: false,
            kind: StepKind::Shell {
                run: vec!["true".to_owned()],
                env: BTreeMap::new(),
            },
        });
        Job {
            display_name: "Dependency seed".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            check_runner: None,
            timeout_minutes: super::super::super::JobTimeout::CRATE,
            needs: vec!["plan".to_owned()],
            condition: Some(super::super::super::DEPENDENCY_SEED_JOB_CONDITION.to_owned()),
            permissions: None,
            cache_mode: None,
            environment: None,
            services: Vec::new(),
            steps,
        }
    }

    #[test]
    fn both_fixed_rust_verifier_calls_remain_admissible_in_a_seed_job() {
        use crate::workflow::RUST_VERIFIER_USES;

        let ids = BTreeSet::from(["plan"]);
        for uses in RUST_VERIFIER_USES {
            verifier_seed_job(uses)
                .validate("dependency-seed", &ids, &Permissions::default(), false)
                .expect("fixed verifier action remains a cache-only seed role");
        }
    }

    #[test]
    fn forged_verifier_role_cannot_admit_a_seed_job() {
        use crate::workflow::{StepKind, StepRole};

        let ids = BTreeSet::from(["plan"]);
        let mut job = verifier_seed_job("./.github/actions/velnor-verify-rust-manifest");
        let Some(step) = job.steps.first_mut() else {
            unreachable!("fixture has a verifier step");
        };
        step.kind = StepKind::Action {
            uses: "./.github/actions/other-verifier".to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        };
        step.role = Some(StepRole::VerifyRustManifest);
        assert!(
            job.validate("dependency-seed", &ids, &Permissions::default(), false)
                .is_err(),
            "forged action path must fail typed role validation"
        );
    }
}
