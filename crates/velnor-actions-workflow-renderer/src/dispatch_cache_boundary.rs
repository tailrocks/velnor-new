//! Prevent cache access before the plan has authenticated dispatch context.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind};

use crate::render::PLAN_JOB_ID;

const DISPATCH_DENY: &str = "github.event_name != 'workflow_dispatch'";

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
            if is_mise_setup_cache(step) {
                disable_mise_cache(step);
            } else if is_cache_access(step) {
                suppress_dispatch(step);
            }
        }
    }
}

fn is_mise_setup_cache(step: &Step) -> bool {
    matches!(&step.kind, StepKind::Action { uses, with, .. }
        if uses.starts_with("jdx/mise-action@")
            && with.get("cache").is_some_and(|value| value == "true"))
}

fn disable_mise_cache(step: &mut Step) {
    let StepKind::Action { with, .. } = &mut step.kind else {
        return;
    };
    with.insert(
        "cache".to_owned(),
        "${{ github.event_name != 'workflow_dispatch' && 'true' || 'false' }}".to_owned(),
    );
}

fn is_cache_access(step: &Step) -> bool {
    if is_mbx_bundle_shell(step) {
        return true;
    }
    let StepKind::Action { uses, .. } = &step.kind else {
        return false;
    };
    uses.starts_with("actions/cache@")
        || uses.starts_with("actions/cache/")
        || uses.starts_with("Swatinem/rust-cache@")
        || uses.starts_with("jdx/mr-boxington-action@")
}

fn is_mbx_bundle_shell(step: &Step) -> bool {
    matches!(
        step.name.as_str(),
        crate::mbx_bundle::MBX_BUNDLE_KEY_NAME
            | crate::mbx_bundle::MBX_BUNDLE_IMPORT_NAME
            | crate::mbx_bundle::MBX_BUNDLE_EXPORT_NAME
    )
}

fn suppress_dispatch(step: &mut Step) {
    let prior = step
        .condition
        .take()
        .unwrap_or_else(|| "success()".to_owned());
    step.condition = Some(format!("({prior}) && {DISPATCH_DENY}"));
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

    fn shell_cache_step(name: &str) -> Step {
        Step {
            name: name.to_owned(),
            condition: None,
            kind: StepKind::Shell {
                run: vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
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
                "mbx-plan".to_owned(),
                job(
                    &[],
                    shell_cache_step(crate::mbx_bundle::MBX_BUNDLE_KEY_NAME),
                ),
            ),
            (
                "crate".to_owned(),
                job(&["plan"], cache_step("Swatinem/rust-cache@sha", "cargo")),
            ),
        ]);

        suppress_unvalidated_cache_access(&mut jobs);

        for id in ["plan", "mbx-plan"] {
            let condition = jobs[id].steps[0]
                .condition
                .as_deref()
                .expect("pre-plan cache condition");
            assert!(condition.contains("github.event_name != 'workflow_dispatch'"));
        }
        let StepKind::Action { with, .. } = &jobs["actionlint"].steps[0].kind else {
            panic!("mise setup remains an action");
        };
        assert!(jobs["actionlint"].steps[0].condition.is_none());
        assert_eq!(
            with.get("cache").map(String::as_str),
            Some("${{ github.event_name != 'workflow_dispatch' && 'true' || 'false' }}"),
            "setup still runs but cache access is denied on dispatch"
        );
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

    #[test]
    fn dispatch_deny_remains_outermost_for_existing_disjunctions() {
        let mut step = cache_step("actions/cache/save@sha", "sources");
        step.condition = Some("(github.event_name != 'workflow_dispatch' || always())".to_owned());
        let mut jobs = BTreeMap::from([("plan".to_owned(), job(&[], step))]);

        suppress_unvalidated_cache_access(&mut jobs);

        assert_eq!(
            jobs["plan"].steps[0].condition.as_deref(),
            Some(
                "((github.event_name != 'workflow_dispatch' || always())) && github.event_name != 'workflow_dispatch'"
            )
        );
    }
}
