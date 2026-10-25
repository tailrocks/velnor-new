use std::collections::BTreeSet;
use std::sync::Arc;

use velnor_actions_contract::Job;
use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};

use super::{caller_condition_for_selection, prepare_job_inner, runner::capture_runner_metadata};
use crate::RenderError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum JobWorkflowSelection {
    LegacyPrototype,
    Explicit(Arc<BTreeSet<String>>),
}

pub(crate) fn pair_is_relocatable(
    hosted_id: &str,
    hosted: &Job,
    local_id: &str,
    local: &Job,
) -> bool {
    let Some(owner) = hosted_id.strip_suffix(HOSTED_SUFFIX) else {
        return false;
    };
    if owner.is_empty() || format!("{owner}{SCALE_SUFFIX}") != local_id {
        return false;
    }
    let ids = BTreeSet::from([hosted_id.to_owned(), local_id.to_owned()]);
    let selection = JobWorkflowSelection::explicit(&ids);
    capture_runner_metadata(hosted_id, hosted).is_ok()
        && capture_runner_metadata(local_id, local).is_ok()
        && prepare_job_inner(hosted_id, hosted, selection.clone()).is_ok()
        && prepare_job_inner(local_id, local, selection).is_ok()
}

impl JobWorkflowSelection {
    pub(crate) fn legacy_prototype() -> Self {
        Self::LegacyPrototype
    }

    pub(crate) fn explicit(ids: &BTreeSet<String>) -> Self {
        Self::Explicit(Arc::new(ids.clone()))
    }

    pub(crate) fn ids(&self) -> Option<&BTreeSet<String>> {
        match self {
            Self::LegacyPrototype => None,
            Self::Explicit(ids) => Some(ids),
        }
    }

    pub(crate) const fn is_legacy_prototype(&self) -> bool {
        matches!(self, Self::LegacyPrototype)
    }

    pub(crate) const fn is_explicit(&self) -> bool {
        matches!(self, Self::Explicit(_))
    }

    pub(crate) fn caller_condition(
        &self,
        job_id: &str,
        condition: Option<&str>,
    ) -> Result<Option<String>, RenderError> {
        match self {
            Self::LegacyPrototype => Ok(condition.map(str::to_owned)),
            Self::Explicit(_) => caller_condition_for_selection(job_id, condition).map(Some),
        }
    }

    pub(crate) fn validate_caller_condition(
        &self,
        job_id: &str,
        condition: Option<&str>,
    ) -> Result<(), RenderError> {
        self.caller_condition(job_id, condition).map(|_| ())
    }
}
