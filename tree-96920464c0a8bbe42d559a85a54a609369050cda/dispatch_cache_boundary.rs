//! Prevent cache access before the plan has authenticated dispatch context.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, Step, StepKind};

const DISPATCH_DENY: &str = "github.event_name != 'workflow_dispatch'";

/// Suppress every cache path until a validated directive controls its use.
///
/// Raw dispatch metadata can deny cache access, but cannot grant it. The
/// current renderer has no cache producer that admits its selected backend
/// object against a validated directive before import, so every cache path
/// stays disabled on workflow dispatch, including downstream jobs.
pub(crate) fn suppress_unvalidated_cache_access(jobs: &mut BTreeMap<String, Job>) {
    for job in jobs.values_mut() {
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
        || uses.starts_with("jdx/mr-boxington-action@")
        || uses == crate::tool_seed::TOOL_SEED_USES
}

fn is_mbx_bundle_shell(step: &Step) -> bool {
    matches!(
        step.name.as_str(),
        crate::mbx_bundle::MBX_CACHE_KEY_NAME
            | crate::mbx_bundle::MBX_BUNDLE_IMPORT_NAME
            | crate::mbx_bundle::MBX_BUNDLE_EXPORT_NAME
    )
}

fn suppress_dispatch(step: &mut Step) {
    if step.name == crate::cache_steps::TOOLS_RESTORE_NAME
        && step.condition.as_deref() == Some(crate::cache_p08::TOOLS_CACHE_RESTORE_CONDITION)
    {
        return;
    }
    if step.condition.as_deref() == Some(DISPATCH_DENY) {
        return;
    }
    let Some(prior) = step.condition.take() else {
        // GitHub applies its implicit success() check when an if expression
        // does not contain a status-check function. Keep the deny predicate
        // short for the common no-condition cache step.
        step.condition = Some(DISPATCH_DENY.to_owned());
        return;
    };
    if prior == "success()" {
        step.condition = Some(DISPATCH_DENY.to_owned());
        return;
    }
    step.condition = Some(format!("({prior}) && {DISPATCH_DENY}"));
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::render::{CONCURRENCY_CANCEL, CONCURRENCY_GROUP, PLAN_JOB_ID, RenderContext};
    use crate::setup::MiseSetup;
    use crate::steps::{RELEASE_COMMIT_ENV, STAGED_BINARY_PREFIX};
    use velnor_actions_contract::{
        Concurrency, Job, JobTimeout, Permissions, Step, StepKind, Trigger, WorkflowIr,
        WorkflowPolicy,
        workflow::{DispatchInput, DispatchInputType, WorkflowDispatch},
    };

    use super::{DISPATCH_DENY, suppress_unvalidated_cache_access};

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
    fn cache_access_stays_disabled_until_a_plan_admits_its_directive() {
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
                job(&[], shell_cache_step(crate::mbx_bundle::MBX_CACHE_KEY_NAME)),
            ),
            (
                "crate".to_owned(),
                job(&["plan"], cache_step("actions/cache/save@sha", "cargo")),
            ),
        ]);

        suppress_unvalidated_cache_access(&mut jobs);

        for id in ["plan", "mbx-plan", "crate"] {
            let condition = jobs[id].steps[0]
                .condition
                .as_deref()
                .expect("pre-plan cache condition");
            assert_eq!(condition, DISPATCH_DENY);
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
    }

    fn acquire_step(staged: &str) -> Result<Step, crate::RenderError> {
        crate::steps::acquire_velnor_step(
            vec![
                "sh".to_owned(),
                "-c".to_owned(),
                format!(
                    "mkdir -p $RUNNER_TEMP/velnor/bin && curl -fsSL \"$VELNOR_ASSET_URL\" -o {staged} && echo \"$VELNOR_ASSET_SHA256  {staged}\" | sha256sum -c - && chmod +x {staged}"
                ),
            ],
            &BTreeMap::from([
                (
                    crate::steps::ASSET_URL_ENV.to_owned(),
                    "https://example.invalid/velnor".to_owned(),
                ),
                (crate::steps::ASSET_SHA_ENV.to_owned(), "a".repeat(64)),
                (RELEASE_COMMIT_ENV.to_owned(), "b".repeat(40)),
            ]),
        )
    }

    fn dispatch_plan_job(staged: &str) -> Result<Job, crate::RenderError> {
        let action = Step {
            name: "Restore MBX objects".to_owned(),
            condition: None,
            kind: StepKind::Action {
                uses: format!("jdx/mr-boxington-action@{}", "a".repeat(40)),
                with: BTreeMap::from([("toolchain".to_owned(), "1.98.1".to_owned())]),
                env: BTreeMap::from([("VELNOR_MBX_VERSION".to_owned(), "1.21.1".to_owned())]),
            },
        };
        Ok(Job {
            display_name: "Velnor Plan".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            timeout_minutes: JobTimeout::CRATE,
            needs: Vec::new(),
            condition: None,
            permissions: None,
            environment: None,
            steps: vec![
                crate::steps::checkout_step(&format!("actions/checkout@{}", "c".repeat(40)))?,
                acquire_step(staged)?,
                action,
                crate::steps::plan_step(),
            ],
        })
    }

    fn dispatch_ir(job: Job) -> WorkflowIr {
        WorkflowIr {
            name: "CI".to_owned(),
            triggers: Trigger {
                pull_request_types: ["opened", "synchronize", "reopened", "ready_for_review"]
                    .iter()
                    .map(ToString::to_string)
                    .collect(),
                push_branches: vec!["main".to_owned()],
                merge_group: true,
                workflow_dispatch: Some(WorkflowDispatch {
                    inputs: vec![DispatchInput {
                        name: "phase".to_owned(),
                        required: true,
                        input_type: DispatchInputType::Choice,
                        choices: vec![
                            "cold".to_owned(),
                            "control".to_owned(),
                            "third".to_owned(),
                            "useful_delta".to_owned(),
                            "warm".to_owned(),
                        ],
                        default: Some("cold".to_owned()),
                    }],
                }),
                schedule: None,
            },
            permissions: Permissions::default(),
            concurrency: Concurrency {
                group: CONCURRENCY_GROUP.to_owned(),
                cancel_in_progress: CONCURRENCY_CANCEL.to_owned(),
            },
            jobs: BTreeMap::from([(PLAN_JOB_ID.to_owned(), job)]),
        }
    }

    fn dispatch_render_context(staged: String) -> RenderContext {
        RenderContext {
            generator_version: "0.1.0".to_owned(),
            runs_on: "ubuntu-26.04".to_owned(),
            staged_binary: staged,
            request_dir: "${{ runner.temp }}/velnor/r1-a1".to_owned(),
            checkout_uses: format!("actions/checkout@{}", "c".repeat(40)),
            validator_commands: Vec::new(),
            candidate: None,
            preseed: false,
            verification_tasks: Vec::new(),
            plan_consumer_env: BTreeMap::new(),
        }
    }

    fn dispatch_mise() -> MiseSetup {
        MiseSetup {
            uses: format!("jdx/mise-action@{}", "d".repeat(40)),
            version: "2026.9.18".to_owned(),
            sha256: "d24fe0bf7e613824ad99f7b8dac3f2b381a37b9f75f84dd250855217095a8de4".to_owned(),
        }
    }

    #[test]
    fn generated_dispatch_plan_gates_mbx_key_and_action_cache_steps()
    -> Result<(), crate::RenderError> {
        let staged = format!("{STAGED_BINARY_PREFIX}0.1.0");
        let ir = dispatch_ir(dispatch_plan_job(&staged)?);
        let context = dispatch_render_context(staged);
        let jobs = crate::render::finalize_jobs(
            &ir,
            WorkflowPolicy::ConsumerV1,
            None,
            &context,
            &dispatch_mise(),
        )?;
        let plan = jobs.get(PLAN_JOB_ID).expect("finalized plan job");
        let key = plan
            .steps
            .iter()
            .find(|step| step.name == crate::mbx_bundle::MBX_CACHE_KEY_NAME)
            .expect("MBX key step");
        assert!(key.condition.as_deref().is_some_and(|condition| {
            condition.contains("github.event_name != 'workflow_dispatch'")
        }));
        let mbx = plan
            .steps
            .iter()
            .find(|step| step.name == "Restore MBX objects")
            .expect("MBX action");
        assert!(mbx.condition.as_deref().is_some_and(|condition| {
            condition.contains("github.event_name != 'workflow_dispatch'")
        }));
        let mise = plan
            .steps
            .iter()
            .find(|step| step.name == crate::setup::SETUP_MISE_NAME)
            .expect("Mise setup");
        let StepKind::Action { with, .. } = &mise.kind else {
            return Err(crate::RenderError::InvalidWorkflow(
                "setup_mise_not_action".to_owned(),
            ));
        };
        assert_eq!(with.get("cache").map(String::as_str), Some("false"));
        Ok(())
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

    #[test]
    fn dispatch_denies_the_local_tool_seed_action() {
        let seed = Step {
            name: crate::tool_seed::TOOL_SEED_NAME.to_owned(),
            condition: Some("always()".to_owned()),
            kind: StepKind::Action {
                uses: crate::tool_seed::TOOL_SEED_USES.to_owned(),
                with: BTreeMap::new(),
                env: BTreeMap::new(),
            },
        };
        let mut jobs = BTreeMap::from([("plan".to_owned(), job(&[], seed))]);

        suppress_unvalidated_cache_access(&mut jobs);

        assert!(
            jobs["plan"].steps[0]
                .condition
                .as_deref()
                .is_some_and(|condition| condition.contains(DISPATCH_DENY))
        );
    }
}
