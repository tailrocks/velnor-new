//! Closed unconditional Pages step sequence.
use super::{NativePagesDeploy, invalid};
use crate::ContractError;
use crate::workflow::{
    ir::Job,
    step::{Step, StepKind},
};
use std::collections::BTreeMap;

impl NativePagesDeploy {
    pub(super) fn validate_steps(&self, job: &Job) -> Result<(), ContractError> {
        let mut sequence = Vec::new();
        let mut admitted = false;
        let mut prepared = Vec::new();
        for step in &job.steps {
            match &step.kind {
                StepKind::SourceBoundHelper { invocation, env } => {
                    if let Some(preparation) = self
                        .preparation
                        .iter()
                        .find(|preparation| step.id.as_ref() == Some(&preparation.step_id))
                    {
                        use crate::workflow::source_helper::SourceBoundOperation::{
                            MiseBootstrap, MiseToolPrepare, NativePagesToolPreparation,
                        };
                        if admitted
                            || invocation != &preparation.invocation
                            || env != &preparation.environment
                            || !matches!(
                                invocation.descriptor().operation(),
                                MiseBootstrap | MiseToolPrepare | NativePagesToolPreparation
                            )
                        {
                            return Err(invalid("foreign_tool_preparation"));
                        }
                        prepared.push(preparation.step_id.as_str());
                        continue;
                    }
                    if step.id.as_ref() != Some(&self.admission_step)
                        || invocation != &self.admission
                        || env != &self.admission_environment()
                        || admitted
                        || sequence.as_slice() != [0]
                    {
                        return Err(invalid("foreign_or_bypassed_admission"));
                    }
                    admitted = true;
                }
                StepKind::Action { uses, with, env } => {
                    let index = self.validate_action(step, uses, with, env)?;
                    if index > 0 && !admitted {
                        return Err(invalid("deployment_before_admission"));
                    }
                    sequence.push(index);
                }
                _ => return Err(invalid("foreign_step_payload")),
            }
        }
        let expected = self
            .preparation
            .iter()
            .map(|preparation| preparation.step_id.as_str())
            .collect::<Vec<_>>();
        if !admitted || sequence != [0, 1, 2, 3] || prepared != expected {
            return Err(invalid("missing_or_duplicate_deployment_step"));
        }
        Ok(())
    }
}

impl NativePagesDeploy {
    fn validate_action(
        &self,
        step: &Step,
        uses: &str,
        with: &BTreeMap<String, String>,
        env: &BTreeMap<String, String>,
    ) -> Result<u8, ContractError> {
        if !env.is_empty() {
            return Err(invalid("action_environment_override"));
        }
        let index = if uses == self.actions.checkout.as_str() {
            if with.get("persist-credentials").map(String::as_str) != Some("false")
                || with.get("ref").map(String::as_str) != Some("${{ github.sha }}")
                || with.keys().any(|key| {
                    !matches!(key.as_str(), "persist-credentials" | "ref" | "fetch-depth")
                })
            {
                return Err(invalid("invalid_source_checkout"));
            }
            0
        } else if uses == self.actions.configure.as_str() && with.is_empty() {
            1
        } else if uses == self.actions.upload.as_str()
            && *with == BTreeMap::from([("path".to_owned(), "public".to_owned())])
        {
            2
        } else if uses == self.actions.deploy.as_str()
            && with.is_empty()
            && step.id.as_ref() == Some(&self.deploy_step)
        {
            3
        } else {
            return Err(invalid("foreign_action_or_inputs"));
        };
        Ok(index)
    }
}
