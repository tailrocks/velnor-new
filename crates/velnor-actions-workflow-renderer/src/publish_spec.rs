// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::{JobTimeout, Step, StepKind, is_valid_branch_name, validate_job_id};

pub use crate::publish_roles::{PublishPermissions, PublishRole};
use crate::{
    RenderError,
    release_permissions::PermissionLevel,
    release_spec::{is_clean_text, validate_environment, validate_repository},
    steps::scan_for_private_subcommands,
};

#[derive(Debug, Clone)]
pub struct PublishStep {
    pub id: Option<String>,
    pub step: Step,
}

#[derive(Debug, Clone)]
pub struct PublishJobSpec {
    pub role: PublishRole,
    pub display_name: String,
    pub runs_on: String,
    pub timeout_minutes: JobTimeout,
    pub needs: Vec<String>,
    pub condition: Option<String>,
    pub environment: Option<String>,
    pub outputs: BTreeMap<String, String>,
    pub permissions: PublishPermissions,
    pub steps: Vec<PublishStep>,
    pub services: Vec<velnor_actions_contract::ServiceContainer>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishTriggers {
    pub branches: Vec<String>,
    pub tag_glob: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PublishWorkflowSpec {
    pub name: String,
    pub repository: String,
    pub triggers: PublishTriggers,
    pub jobs: BTreeMap<String, PublishJobSpec>,
    pub staged_binary: Option<String>,
}

const BUILD_INTERNAL_OPS: [&str; 3] = [
    velnor_actions_contract::GRADLE_RUN_OPERATION,
    velnor_actions_contract::PRIVATE_FETCH_OPERATION,
    velnor_actions_contract::PRIVATE_REVOKE_OPERATION,
];

impl PublishJobSpec {
    pub fn validate_shape(&self, id: &str) -> Result<(), RenderError> {
        let bad = |what: &str| RenderError::InvalidWorkflow(format!("{what}:{id}"));
        if !is_clean_text(&self.display_name, 128) {
            return Err(bad("bad_job_display"));
        }
        scan_for_private_subcommands(&self.display_name)?;
        crate::guard::validate_runs_on(&self.runs_on)?;
        if self.steps.is_empty() {
            return Err(bad("empty_steps"));
        }
        if !self.services.is_empty() && self.role != PublishRole::ArtifactBuild {
            return Err(bad("services_in_privileged_role"));
        }
        if self.permissions != PublishPermissions::expected(self.role) {
            return Err(bad("permission_matrix"));
        }
        let writes = self.permissions.contents == PermissionLevel::Write
            || self.permissions.packages == PermissionLevel::Write;
        if writes != self.environment.is_some() {
            return Err(bad("environment_binding"));
        }
        if let Some(environment) = &self.environment {
            validate_environment(environment)?;
        }
        if let Some(condition) = &self.condition {
            if !is_clean_text(condition, 512) || condition.contains("secrets.") {
                return Err(bad("bad_condition"));
            }
            scan_for_private_subcommands(condition)?;
        }
        for step in &self.steps {
            self.validate_step(id, step)?;
        }
        self.validate_outputs(id)
    }

    fn validate_step(&self, id: &str, step: &PublishStep) -> Result<(), RenderError> {
        let bad = |what: &str| RenderError::InvalidWorkflow(format!("{what}:{id}"));
        if !is_clean_text(&step.step.name, 128) {
            return Err(bad("bad_step_name"));
        }
        scan_for_private_subcommands(&step.step.name)?;
        if let Some(step_id) = &step.id
            && !is_step_id(step_id)
        {
            return Err(bad("bad_step_id"));
        }
        match &step.step.kind {
            StepKind::Internal { operation, .. }
                if self.role == PublishRole::ArtifactBuild
                    && BUILD_INTERNAL_OPS.contains(&operation.as_str()) => {}
            StepKind::Internal { .. } | StepKind::TaskExecution { .. } => {
                return Err(bad("publish_internal_op"));
            }
            StepKind::Action { uses, .. }
                if self.role.forbids_checkout() && uses.starts_with("actions/checkout@") =>
            {
                return Err(bad("checkout_in_privileged_role"));
            }
            _ => {}
        }
        Ok(())
    }

    fn validate_outputs(&self, id: &str) -> Result<(), RenderError> {
        let bad = || RenderError::InvalidWorkflow(format!("bad_job_output:{id}"));
        for (name, value) in &self.outputs {
            let slug = |s: &str| {
                !s.is_empty()
                    && s.bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
            };
            let inner = value
                .strip_prefix("${{ steps.")
                .and_then(|rest| rest.strip_suffix(" }}"))
                .ok_or_else(bad)?;
            let (step_id, output) = inner.split_once(".outputs.").ok_or_else(bad)?;
            let declared = self.steps.iter().any(|s| s.id.as_deref() == Some(step_id));
            if !slug(name) || !slug(output) || !declared {
                return Err(bad());
            }
        }
        Ok(())
    }
}

fn is_step_id(id: &str) -> bool {
    id.starts_with(|c: char| c.is_ascii_lowercase())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

impl PublishWorkflowSpec {
    pub fn validate(&self) -> Result<(), RenderError> {
        if !is_clean_text(&self.name, 128) {
            return Err(RenderError::InvalidWorkflow("bad_publish_name".to_owned()));
        }
        scan_for_private_subcommands(&self.name)?;
        validate_repository(&self.repository)?;
        self.validate_triggers()?;
        self.validate_staged_binary()?;
        let mut authorities = BTreeMap::new();
        for (id, job) in &self.jobs {
            validate_job_id(id).map_err(RenderError::Contract)?;
            job.validate_shape(id)?;
            if job.role.is_authority() && authorities.insert(job.role, id.as_str()).is_some() {
                return Err(RenderError::InvalidWorkflow(
                    "publish_authority_count".to_owned(),
                ));
            }
        }
        if authorities.is_empty() {
            return Err(RenderError::InvalidWorkflow(
                "publish_authority_count".to_owned(),
            ));
        }
        self.check_needs(&authorities)
    }

    fn validate_staged_binary(&self) -> Result<(), RenderError> {
        let internal = self.jobs.values().any(|job| {
            job.steps
                .iter()
                .any(|step| matches!(step.step.kind, StepKind::Internal { .. }))
        });
        match (&self.staged_binary, internal) {
            (Some(path), true) if path.starts_with("$RUNNER_TEMP/velnor/bin/velnor-actions-") => {
                Ok(())
            }
            (None, false) => Ok(()),
            _ => Err(RenderError::InvalidWorkflow(
                "publish_staged_binary".to_owned(),
            )),
        }
    }

    fn validate_triggers(&self) -> Result<(), RenderError> {
        let triggers = &self.triggers;
        if triggers.branches.is_empty() && triggers.tag_glob.is_none() {
            return Err(RenderError::InvalidWorkflow(
                "no_publish_trigger".to_owned(),
            ));
        }
        let unique: BTreeSet<&String> = triggers.branches.iter().collect();
        if triggers.branches.len() > 8 || unique.len() != triggers.branches.len() {
            return Err(RenderError::InvalidWorkflow(
                "bad_publish_branches".to_owned(),
            ));
        }
        for branch in &triggers.branches {
            if !is_valid_branch_name(branch) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "bad_push_branch:{branch}"
                )));
            }
            scan_for_private_subcommands(branch)?;
        }
        if let Some(glob) = &triggers.tag_glob {
            let plain =
                |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'*' | b'[' | b']' | b'.');
            if !is_clean_text(glob, 128) || !glob.bytes().all(plain) {
                return Err(RenderError::InvalidWorkflow(format!("bad_tag_glob:{glob}")));
            }
        }
        Ok(())
    }

    fn check_needs(&self, authorities: &BTreeMap<PublishRole, &str>) -> Result<(), RenderError> {
        for (id, job) in &self.jobs {
            if job.role.is_authority() {
                if !job.needs.is_empty() {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "authority_shape:{id}"
                    )));
                }
                continue;
            }
            for need in &job.needs {
                let Some(target) = self.jobs.get(need) else {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "unknown_need:{id}:{need}"
                    )));
                };
                if target.role.rank() >= job.role.rank() {
                    return Err(RenderError::InvalidWorkflow(format!(
                        "backward_need:{id}:{need}"
                    )));
                }
            }
        }
        for (id, job) in &self.jobs {
            let Some(gate) = job.role.gate() else {
                continue;
            };
            let gated = authorities
                .get(&gate)
                .is_some_and(|authority| self.reaches(id, authority));
            if !gated {
                return Err(RenderError::InvalidWorkflow(format!("ungated_job:{id}")));
            }
        }
        Ok(())
    }

    fn reaches(&self, from: &str, target: &str) -> bool {
        let mut stack = vec![from];
        let mut seen = BTreeSet::new();
        while let Some(current) = stack.pop() {
            if current == target {
                return true;
            }
            if seen.insert(current)
                && let Some(job) = self.jobs.get(current)
            {
                stack.extend(job.needs.iter().map(String::as_str));
            }
        }
        false
    }
}
