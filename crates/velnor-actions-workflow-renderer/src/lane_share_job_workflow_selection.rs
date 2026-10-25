use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::workflow::lanes::{HOSTED_SUFFIX, SCALE_SUFFIX};

use super::job_workflow_extract::{self, JobWorkflowSelection, PreparedJob, pair_is_relocatable};
use crate::RenderError;

pub(super) fn prepare(
    jobs: &BTreeMap<String, velnor_actions_contract::Job>,
    source_ids: &BTreeSet<String>,
) -> Result<BTreeMap<String, PreparedJob>, RenderError> {
    validate_complete_pairs(source_ids)?;
    for source_id in source_ids {
        if !jobs.contains_key(source_id) {
            return Err(invalid_selection(source_id, "selected_source_job_missing"));
        }
    }
    for hosted_id in source_ids
        .iter()
        .filter(|source_id| source_id.ends_with(HOSTED_SUFFIX))
    {
        let Some(owner) = hosted_id.strip_suffix(HOSTED_SUFFIX) else {
            return Err(invalid_selection(hosted_id, "hosted_suffix_changed"));
        };
        let local_id = format!("{owner}{SCALE_SUFFIX}");
        let hosted = &jobs[hosted_id];
        let local = &jobs[&local_id];
        if !pair_is_relocatable(hosted_id, hosted, &local_id, local) {
            return Err(invalid_selection(
                hosted_id,
                "selected_source_pair_not_lowerable",
            ));
        }
    }
    let selection = JobWorkflowSelection::explicit(source_ids);
    source_ids
        .iter()
        .map(|source_id| {
            let job = jobs
                .get(source_id)
                .ok_or_else(|| invalid_selection(source_id, "selected_source_job_missing"))?;
            let prepared =
                job_workflow_extract::prepare_job_inner(source_id, job, selection.clone())?;
            Ok((source_id.clone(), prepared))
        })
        .collect()
}

fn validate_complete_pairs(source_ids: &BTreeSet<String>) -> Result<(), RenderError> {
    for source_id in source_ids {
        let counterpart = if let Some(owner) = source_id.strip_suffix(HOSTED_SUFFIX) {
            format!("{owner}{SCALE_SUFFIX}")
        } else if let Some(owner) = source_id.strip_suffix(SCALE_SUFFIX) {
            format!("{owner}{HOSTED_SUFFIX}")
        } else {
            return Err(invalid_selection(source_id, "lane_suffix_missing"));
        };
        if !source_ids.contains(&counterpart) {
            return Err(invalid_selection(source_id, "lane_pair_incomplete"));
        }
    }
    Ok(())
}

fn invalid_selection(source_id: &str, problem: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!(
        "job_workflow_extract:{source_id}:selection:{problem}"
    ))
}

#[cfg(test)]
mod tests {
    use super::prepare;
    use crate::lane_share::job_workflow_extract::{PROTOTYPE_JOB_ID, PROTOTYPE_LOCAL_JOB_ID};
    use velnor_actions_contract::{Job, JobTimeout};

    fn job(runs_on: &str) -> Job {
        Job {
            display_name: "Rust CLI".to_owned(),
            runs_on: runs_on.to_owned(),
            check_runner: None,
            timeout_minutes: JobTimeout::CRATE,
            needs: vec!["plan".to_owned()],
            condition: None,
            permissions: None,
            cache_mode: None,
            environment: None,
            services: Vec::new(),
            steps: Vec::new(),
        }
    }

    #[test]
    fn explicit_empty_selection_does_not_use_the_prototype_fallback() {
        let jobs = std::collections::BTreeMap::from([
            (PROTOTYPE_JOB_ID.to_owned(), job("ubuntu-26.04")),
            (
                PROTOTYPE_LOCAL_JOB_ID.to_owned(),
                job("scale-set:velnor+ubuntu-26.04-scale-set"),
            ),
        ]);

        assert!(
            prepare(&jobs, &std::collections::BTreeSet::new())
                .expect("explicit empty selection")
                .is_empty()
        );
        assert_eq!(
            super::super::prepare(&jobs, None)
                .expect("legacy fallback")
                .len(),
            1
        );
    }

    #[test]
    fn explicit_selection_rejects_partial_and_unsupported_pairs() {
        let ids = std::collections::BTreeSet::from([PROTOTYPE_JOB_ID.to_owned()]);
        let jobs =
            std::collections::BTreeMap::from([(PROTOTYPE_JOB_ID.to_owned(), job("ubuntu-26.04"))]);
        assert!(prepare(&jobs, &ids).is_err());

        let mut jobs = std::collections::BTreeMap::from([
            (PROTOTYPE_JOB_ID.to_owned(), job("ubuntu-26.04")),
            (
                PROTOTYPE_LOCAL_JOB_ID.to_owned(),
                job("scale-set:velnor+ubuntu-26.04-scale-set"),
            ),
        ]);
        jobs.get_mut(PROTOTYPE_LOCAL_JOB_ID)
            .expect("local source")
            .needs
            .push("seed".to_owned());
        let ids = std::collections::BTreeSet::from([
            PROTOTYPE_JOB_ID.to_owned(),
            PROTOTYPE_LOCAL_JOB_ID.to_owned(),
        ]);
        assert!(prepare(&jobs, &ids).is_err());
    }
}
