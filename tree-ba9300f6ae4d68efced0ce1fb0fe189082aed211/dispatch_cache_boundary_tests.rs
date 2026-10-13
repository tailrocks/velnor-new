use std::collections::BTreeMap;

use crate::render::{CONCURRENCY_CANCEL, CONCURRENCY_GROUP, PLAN_JOB_ID, RenderContext};
use crate::setup::MiseSetup;
use crate::steps::{RELEASE_COMMIT_ENV, STAGED_BINARY_PREFIX};
use velnor_actions_contract::{
    Concurrency, Job, JobTimeout, Permissions, Step, StepKind, Trigger, WorkflowIr, WorkflowPolicy,
    workflow::{DispatchInput, DispatchInputType, WorkflowDispatch},
};

use super::{DISPATCH_DENY, suppress_unvalidated_cache_access};

pub(crate) fn cache_step(uses: &str, name: &str) -> Step {
    Step {
        name: name.to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with: BTreeMap::from([("cache".to_owned(), "true".to_owned())]),
            env: BTreeMap::new(),
        },
    }
}

pub(crate) fn job(needs: &[&str], step: Step) -> Job {
    Job {
        display_name: "cache job".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
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
            "crate".to_owned(),
            job(&["plan"], cache_step("actions/cache/save@sha", "cargo")),
        ),
    ]);

    suppress_unvalidated_cache_access(&mut jobs);

    for id in ["plan", "crate"] {
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
        Some("false"),
        "setup still runs but cache access is denied on dispatch"
    );
    assert_eq!(with.get("cache_save").map(String::as_str), Some("false"));
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
        id: None,
        role: None,
        condition: None,
            kind: StepKind::Action {
                uses: format!("jdx/mr-boxington-action@{}", "a".repeat(40)),
                with: BTreeMap::from([("toolchain".to_owned(), "1.98.1".to_owned())]),
                env: BTreeMap::from([
                    ("VELNOR_MBX_VERSION".to_owned(), "1.21.1".to_owned()),
                    (
                        crate::cache_steps::MBX_CACHE_MODE_ENV.to_owned(),
                        "${{ github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true && 'write' || 'read' }}".to_owned(),
                    ),
                ]),
            },
        };
    Ok(Job {
        display_name: "Velnor Plan".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        check_runner: None,
        timeout_minutes: JobTimeout::CRATE,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![
            crate::steps::checkout_step(&format!("actions/checkout@{}", "c".repeat(40)))?,
            acquire_step(staged)?,
            action,
            Step {
                name: "Build".to_owned(),
                id: None,
                role: None,
                condition: None,
                kind: StepKind::Shell {
                    run: vec![
                        "mise".to_owned(),
                        "exec".to_owned(),
                        "rust@1.89.0".to_owned(),
                        "--".to_owned(),
                        "cargo".to_owned(),
                        "build".to_owned(),
                    ],
                    env: BTreeMap::from(
                        crate::toolchain_env::STEP_CREDENTIAL_DENYLIST
                            .map(|key| (key.to_owned(), String::new())),
                    ),
                },
            },
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
        workflow_tasks: Vec::new(),
        pull_request_cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        plan_consumer_env: BTreeMap::new(),
    }
}

fn dispatch_mise() -> MiseSetup {
    MiseSetup {
        uses: format!("jdx/mise-action@{}", "d".repeat(40)),
        version: "2026.10.5".to_owned(),
        sha256: "8a223b5f8ca71100220a3e5bef259614c348e7b1d80e6b15c2a9c9aa3affe5e4".to_owned(),
    }
}

#[test]
fn generated_dispatch_plan_gates_native_mbx_and_tools_cache_steps() -> Result<(), crate::RenderError>
{
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
    assert_native_mbx_step(plan);
    assert_seed_and_mise_steps(plan)?;
    assert_rendered_dispatch_guard(&ir, &context)?;
    Ok(())
}

fn assert_native_mbx_step(plan: &Job) {
    let mbx = plan
        .steps
        .iter()
        .find(|step| step.name == "Restore MBX objects")
        .expect("MBX action");
    assert!(mbx.condition.as_deref().is_some_and(|condition| {
        condition.contains("github.event_name != 'workflow_dispatch'")
    }));
    let native_mode = match &mbx.kind {
        StepKind::Action { env, .. } => env
            .get(crate::cache_steps::MBX_CACHE_MODE_ENV)
            .map(String::as_str),
        _ => None,
    };
    assert_eq!(
        native_mode,
        Some(
            "${{ github.event_name == 'push' && github.ref == format('refs/heads/{0}', github.event.repository.default_branch) && github.ref_protected == true && 'write' || 'read' }}"
        ),
        "native object writes remain limited to protected default-branch pushes"
    );
}

fn assert_seed_and_mise_steps(plan: &Job) -> Result<(), crate::RenderError> {
    // V2 carries the tool seed inside the runtime-identity prelude; the
    // boundary still gates the MBX object step below. The prelude digest is
    // static and content-bound, so dispatch cannot smuggle a foreign seed.
    let prelude = plan
        .steps
        .iter()
        .find(|step| step.role == Some(velnor_actions_contract::StepRole::ToolsCacheIdentity))
        .expect("V2 tools-cache identity prelude");
    let StepKind::Action { with, .. } = &prelude.kind else {
        return Err(crate::RenderError::InvalidWorkflow(
            "tools_cache_prelude_not_action".to_owned(),
        ));
    };
    let digest = with
        .get(crate::cache_p08::TOOLS_CACHE_IDENTITY_DIGEST_INPUT)
        .map(String::as_str)
        .unwrap_or_default();
    assert!(
        digest.len() == 64
            && digest.bytes().all(|byte| byte.is_ascii_hexdigit()
                && (byte.is_ascii_digit() || byte.is_ascii_lowercase())),
        "prelude binds the exact static runtime digest"
    );
    assert!(
        plan.steps
            .iter()
            .all(|step| !step.name.contains("MBX bundle")),
        "the retired manual transport is absent"
    );
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

fn assert_rendered_dispatch_guard(
    ir: &WorkflowIr,
    context: &RenderContext,
) -> Result<(), crate::RenderError> {
    let rendered = crate::render::render_workflow_ir_strict_shared(
        ir,
        WorkflowPolicy::ConsumerV1,
        None,
        context,
        &dispatch_mise(),
    )?;
    assert!(
        rendered
            .yaml
            .contains(crate::cache_p08::TOOLS_CACHE_IDENTITY_NAME)
    );
    assert!(
        rendered
            .yaml
            .contains("github.event_name != 'workflow_dispatch'")
    );
    Ok(())
}

#[test]
fn empty_seed_key_exits_before_the_trusted_image_tree_is_inspected() {
    let script = crate::tool_seed::tool_seed_action_script(crate::tool_seed::SEED_ROOT)
        .expect("fixed seed script");
    let disabled = script.find("if [ -z \"$key\" ]").expect("empty key gate");
    let trust = script
        .find("trusted_seed_is_trusted")
        .expect("trusted tree check");
    assert!(disabled < trust, "empty key must exit before seed access");
}

#[path = "dispatch_cache_boundary_seed_tests.rs"]
mod seed_tests;
