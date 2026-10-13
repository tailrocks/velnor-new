//! Prevent cache access before the plan has authenticated dispatch context.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind};

use crate::render::PLAN_JOB_ID;

/// Suppress every cache action in jobs that cannot consume validated plan outputs.
///
/// Raw event metadata can deny access before planning, but it never grants a
/// writer. Jobs downstream of Plan receive their read/write policy from the
/// validated plan outputs and keep their cache actions for that later gate.
pub(crate) fn suppress_unvalidated_cache_access(jobs: &mut BTreeMap<String, Job>) {
    for (id, job) in jobs {
        let has_plan_outputs =
            id != PLAN_JOB_ID && job.needs.iter().any(|need| need == PLAN_JOB_ID);
        if has_plan_outputs {
            continue;
        }
        for step in &mut job.steps {
            if is_cache_access(step) {
                suppress_dispatch(step);
            }
        }
    }
}

fn is_cache_access(step: &Step) -> bool {
    let StepKind::Action { uses, with, .. } = &step.kind else {
        return false;
    };
    uses.starts_with("actions/cache@")
        || uses.starts_with("actions/cache/")
        || uses.starts_with("Swatinem/rust-cache@")
        || uses.starts_with("jdx/mr-boxington-action@")
        || (uses.starts_with("jdx/mise-action@")
            && with.get("cache").is_some_and(|value| value == "true"))
}

fn suppress_dispatch(step: &mut Step) {
    let prior = step
        .condition
        .take()
        .unwrap_or_else(|| "success()".to_owned());
    step.condition = Some(format!(
        "({prior}) && github.event_name != 'workflow_dispatch'"
    ));
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use velnor_actions_contract::{Job, JobTimeout, Step, StepKind};

    use super::suppress_unvalidated_cache_access;

    fn cache_step(uses: &str, name: &str) -> Step {
        Step {
            name: name.to_owned(),
            condition: None,
            kind: StepKind::Action {
                uses: uses.to_owned(),
                with: BTreeMap::from([("cache".to_owned(), "true".to_owned())]),
                env: BTreeMap::new(),
            },
        }
    }

    fn job(needs: &[&str], step: Step) -> Job {
        Job {
            display_name: "cache job".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            timeout_minutes: JobTimeout::CRATE,
            needs: needs.iter().map(|need| (*need).to_owned()).collect(),
            condition: None,
            permissions: None,
            environment: None,
            steps: vec![step],
        }
    }

    #[test]
    fn preplan_cache_access_is_suppressed_but_validated_downstream_is_untouched() {
        let mut jobs = BTreeMap::from([
            (
                "plan".to_owned(),
                job(&[], cache_step("actions/cache/restore@sha", "sources")),
            ),
            (
                "actionlint".to_owned(),
                job(&[], cache_step("jdx/mise-action@sha", "tools")),
            ),
            (
                "crate".to_owned(),
                job(&["plan"], cache_step("Swatinem/rust-cache@sha", "cargo")),
            ),
        ]);

        suppress_unvalidated_cache_access(&mut jobs);

        for id in ["plan", "actionlint"] {
            let condition = jobs[id].steps[0]
                .condition
                .as_deref()
                .expect("pre-plan cache condition");
            assert!(condition.contains("github.event_name != 'workflow_dispatch'"));
        }
        assert!(jobs["crate"].steps[0].condition.is_none());
    }

    #[test]
    fn suppression_preserves_existing_cache_step_condition() {
        let mut step = cache_step("actions/cache/save@sha", "sources");
        step.condition = Some("success() && github.event_name == 'push'".to_owned());
        let mut jobs = BTreeMap::from([("plan".to_owned(), job(&[], step))]);

        suppress_unvalidated_cache_access(&mut jobs);

        assert_eq!(
            jobs["plan"].steps[0].condition.as_deref(),
            Some(
                "(success() && github.event_name == 'push') && github.event_name != 'workflow_dispatch'"
            )
        );
    }
}
