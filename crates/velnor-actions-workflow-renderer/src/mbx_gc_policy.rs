//! Job-level GC policy for jobs that restore or publish MBX objects.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::Job;

/// Return hosted jobs where the MBX action's collector is disabled by default.
///
/// The renderer attaches this policy before lane sharing extracts steps into
/// composites, so all hosted consuming steps inherit the job environment.
pub(crate) fn jobs_with_hosted_mbx_objects(
    jobs: &BTreeMap<String, Job>,
    default_label: &str,
) -> BTreeSet<String> {
    jobs.iter()
        .filter(|(_, job)| {
            crate::cache_steps::has_hosted_cache_target(job, default_label)
                && job.steps.iter().any(crate::cache_steps::is_mbx_action)
        })
        .map(|(id, _)| id.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use velnor_actions_contract::config::{SCALE_SET_NAME, ScaleSetSelector, VELNOR_LABEL};
    use velnor_actions_contract::{Job, JobTimeout, Step};

    use super::jobs_with_hosted_mbx_objects;
    use crate::steps::mbx_objects_step;

    fn mbx_step() -> Step {
        let uses = format!("jdx/mr-boxington-action@{}", "a".repeat(40));
        mbx_objects_step(&uses, false, "1.21.1").expect("MBX action")
    }

    fn job(runs_on: &str) -> Job {
        Job {
            display_name: runs_on.to_owned(),
            runs_on: runs_on.to_owned(),
            timeout_minutes: JobTimeout::VALIDATOR,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            steps: vec![mbx_step()],
        }
    }

    #[test]
    fn macos_is_hosted_and_mutable_selectors_do_not_get_gc_policy() {
        let scale_set = ScaleSetSelector::try_new(
            SCALE_SET_NAME,
            &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
        )
        .expect("scale-set selector")
        .token();
        let jobs = BTreeMap::from([
            ("macos".to_owned(), job("macos-15")),
            ("scale".to_owned(), job(&scale_set)),
            ("selfhosted".to_owned(), job("[self-hosted, Linux]")),
        ]);
        let selected = jobs_with_hosted_mbx_objects(&jobs, "ubuntu-26.04");
        assert_eq!(selected.len(), 1);
        assert!(selected.contains("macos"));
    }
}
