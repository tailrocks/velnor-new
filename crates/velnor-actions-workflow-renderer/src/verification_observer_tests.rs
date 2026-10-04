//! Negative proof for native observer and simulation payload closure.

use super::*;
use velnor_actions_contract::workflow::ir::{DispatchInput, DispatchInputType, WorkflowDispatch};
use velnor_actions_contract::{
    Job, JobTimeout, PermissionLevel, Permissions, ToolCacheDomain, Trigger,
};

fn triggers(dispatch: bool) -> Trigger {
    Trigger {
        pull_request_types: Vec::new(),
        push_tags: Vec::new(),
        push_branches: vec!["main".to_owned()],
        merge_group: true,
        schedule: None,
        workflow_dispatch: dispatch.then(|| WorkflowDispatch {
            inputs: vec![DispatchInput {
                name: "simulate_failure".to_owned(),
                input_type: DispatchInputType::Boolean,
                description: None,
                options: Vec::new(),
                required: false,
                default: Some("false".to_owned()),
            }],
        }),
    }
}

pub(super) fn tools() -> DeliveryToolContext {
    DeliveryToolContext {
        mise: crate::setup::fixture::mise_setup("2026.10.0", &"b".repeat(64)),
        python_version: "3.14.0".to_owned(),
        gh_version: "2.102.0".to_owned(),
        preparation: preparation_record(),
    }
}

pub(super) fn preparation_record() -> velnor_actions_contract::CompiledSourceHelper {
    let source =
        velnor_actions_contract::generated_source("0.1.0", "exit 0\n").expect("preparation source");
    let operation = velnor_actions_contract::SourceBoundOperation::MiseToolPrepare;
    let descriptor = velnor_actions_contract::SourceBoundHelper::compiled(
        operation,
        operation.path(),
        &velnor_actions_contract::compiled_source_sha256(source.as_bytes()),
    )
    .expect("preparation descriptor");
    let invocation = velnor_actions_contract::HelperInvocation::compiled(
        descriptor,
        Vec::new(),
        vec!["gh@2.102.0".to_owned(), "python@3.14.0".to_owned()],
    )
    .expect("preparation invocation");
    velnor_actions_contract::CompiledSourceHelper::compiled(invocation, source)
        .expect("preparation record")
}

pub(super) fn observer_record(
    repository: &str,
    branch: &str,
) -> velnor_actions_contract::CompiledSourceHelper {
    observer_record_with_args(repository, branch, Vec::new())
}

pub(super) fn observer_record_with_args(
    repository: &str,
    branch: &str,
    args: Vec<String>,
) -> velnor_actions_contract::CompiledSourceHelper {
    use velnor_actions_contract::workflow::native_tools::{
        CompiledNativeExecRecipe, NativeCredentialScope,
    };
    let source = observer_source("0.1.0").expect("observer source");
    let operation = velnor_actions_contract::SourceBoundOperation::VerificationObserver;
    let descriptor = velnor_actions_contract::SourceBoundHelper::compiled(
        operation,
        operation.path(),
        &velnor_actions_contract::compiled_source_sha256(source.as_bytes()),
    )
    .expect("observer descriptor");
    let invocation =
        velnor_actions_contract::HelperInvocation::compiled(descriptor, args, Vec::new())
            .expect("observer invocation");
    let recipe_environment = BTreeMap::from([
        ("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned()),
        ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
    ]);
    let recipe = CompiledNativeExecRecipe::compiled_for_scope(
        vec![
            "/usr/bin/env".to_owned(),
            "-i".to_owned(),
            "GH_TOKEN=${GH_TOKEN}".to_owned(),
            "PATH=/usr/bin:/bin".to_owned(),
            "gh@2.102.0".to_owned(),
            "python@3.14.0".to_owned(),
            "--".to_owned(),
        ],
        recipe_environment,
        vec!["gh@2.102.0".to_owned(), "python@3.14.0".to_owned()],
        NativeCredentialScope::GithubIssueWrite,
    )
    .expect("observer recipe");
    let environment = BTreeMap::from([
        ("APPROVED_REPOSITORY".to_owned(), repository.to_owned()),
        (
            "GITHUB_REPOSITORY".to_owned(),
            "${{ github.repository }}".to_owned(),
        ),
        ("DEFAULT_BRANCH".to_owned(), branch.to_owned()),
        ("REF".to_owned(), "${{ github.ref }}".to_owned()),
        (
            "REF_PROTECTED".to_owned(),
            "${{ github.ref_protected }}".to_owned(),
        ),
        (
            "EVENT_NAME".to_owned(),
            "${{ github.event_name }}".to_owned(),
        ),
        ("SOURCE_SHA".to_owned(), "${{ github.sha }}".to_owned()),
        ("RUN_ID".to_owned(), "${{ github.run_id }}".to_owned()),
        (
            "RUN_ATTEMPT".to_owned(),
            "${{ github.run_attempt }}".to_owned(),
        ),
        (
            "REQUIRED_RESULT".to_owned(),
            "${{ needs.required.result }}".to_owned(),
        ),
    ]);
    velnor_actions_contract::CompiledSourceHelper::compiled(invocation, source)
        .expect("observer record")
        .with_environment(environment)
        .with_execution_recipe(recipe)
        .expect("observer recipe binding")
}

fn job(steps: Vec<Step>) -> Job {
    Job {
        cache_mode: None,
        display_name: "Nightly failure observer".to_owned(),
        runs_on: "ubuntu-24.04".to_owned(),
        timeout_minutes: JobTimeout::VALIDATOR,
        needs: vec![crate::render::FINAL_JOB_ID.to_owned()],
        condition: None,
        permissions: None,
        tool_producer: None,
        mbx_producer: None,
        source_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        environment: None,
        steps,
    }
}

pub(super) fn observer_jobs() -> BTreeMap<String, Job> {
    let tools = tools();
    let record = observer_record("tailrocks/velnor-new", "main");
    let mut steps = observer_setup_steps(&tools, "ubuntu-26.04").expect("fixed setup");
    steps.push(
        observer_step(
            "tailrocks/velnor-new",
            "main",
            crate::render::FINAL_JOB_ID,
            &record,
        )
        .expect("fixed helper"),
    );
    let mut observer = job(steps);
    observer.condition = Some(
        velnor_actions_contract::workflow::observer::observer_condition(
            "tailrocks/velnor-new",
            "main",
        ),
    );
    observer.permissions = Some(Permissions {
        contents: PermissionLevel::None,
        pull_requests: PermissionLevel::None,
        id_token: PermissionLevel::None,
        actions: PermissionLevel::None,
        issues: PermissionLevel::Write,
        pages: PermissionLevel::None,
        attestations: PermissionLevel::None,
    });
    observer.environment = Some("verification-alerts".to_owned());
    BTreeMap::from([("verification-observer".to_owned(), observer)])
}

pub(super) fn records() -> Vec<velnor_actions_contract::CompiledSourceHelper> {
    vec![
        tools()
            .mise
            .bootstrap(ToolCacheDomain::Full, "ubuntu-26.04")
            .expect("bootstrap")
            .helper
            .clone(),
        tools().preparation,
        observer_record("tailrocks/velnor-new", "main"),
    ]
}

#[test]
fn observer_rejects_arguments_and_environment_extensions() {
    let with_args = observer_record_with_args(
        "tailrocks/velnor-new",
        "main",
        vec!["unexpected".to_owned()],
    );
    assert!(
        observer_step(
            "tailrocks/velnor-new",
            "main",
            crate::render::FINAL_JOB_ID,
            &with_args,
        )
        .is_err()
    );

    let mut environment = observer_record("tailrocks/velnor-new", "main")
        .environment()
        .clone();
    environment.insert("UNEXPECTED".to_owned(), "value".to_owned());
    let extended = observer_record("tailrocks/velnor-new", "main").with_environment(environment);
    assert!(
        observer_step(
            "tailrocks/velnor-new",
            "main",
            crate::render::FINAL_JOB_ID,
            &extended,
        )
        .is_err()
    );
}

#[test]
fn reconstructed_observer_rejects_mutable_program_and_credentials() {
    let records = records();
    assert!(
        validate_observer_jobs(
            &observer_jobs(),
            &triggers(false),
            &records,
            env!("CARGO_PKG_VERSION"),
        )
        .is_ok()
    );
    for mutation in 0..6 {
        let mut jobs = observer_jobs();
        let observer = jobs.get_mut("verification-observer").expect("observer");
        match mutation {
            0 => {
                let StepKind::SourceBoundHelper { env, .. } = &mut observer.steps[2].kind else {
                    panic!("helper shell");
                };
                env.insert("SOURCE_SHA".to_owned(), "bad".to_owned());
            }
            1 => {
                let StepKind::SourceBoundHelper { env, .. } = &mut observer.steps[2].kind else {
                    panic!("helper shell");
                };
                env.insert("GITHUB_TOKEN".to_owned(), "${{ github.token }}".to_owned());
            }
            2 => {
                let StepKind::SourceBoundHelper { env, .. } = &mut observer.steps[0].kind else {
                    panic!("setup helper");
                };
                env.insert("MISE_DATA_DIR".to_owned(), "changed".to_owned());
            }
            3 => observer.steps[2].condition = Some("always()".to_owned()),
            4 => observer.steps.push(
                crate::shell_step(
                    "Repository command",
                    vec!["true".to_owned()],
                    BTreeMap::new(),
                )
                .expect("shell"),
            ),
            _ => {
                let StepKind::SourceBoundHelper { .. } = &mut observer.steps[1].kind else {
                    panic!("install shell");
                };
                observer.steps[1] = observer.steps[0].clone();
            }
        }
        assert!(
            validate_observer_jobs(&jobs, &triggers(false), &records, env!("CARGO_PKG_VERSION"),)
                .is_err(),
            "mutation {mutation}"
        );
    }
}

fn simulation_jobs() -> BTreeMap<String, Job> {
    let mut jobs = observer_jobs();
    let plan = job(vec![
        simulation_step().expect("simulation"),
        crate::steps::plan_step(),
    ]);
    let mut required = job(Vec::new());
    required.needs = vec![crate::render::PLAN_JOB_ID.to_owned()];
    jobs.insert(crate::render::PLAN_JOB_ID.to_owned(), plan);
    jobs.insert(crate::render::FINAL_JOB_ID.to_owned(), required);
    jobs
}

#[test]
fn simulation_requires_exact_failure_before_planning_and_boolean_dispatch() {
    let records = records();
    assert!(
        validate_observer_jobs(
            &simulation_jobs(),
            &triggers(true),
            &records,
            env!("CARGO_PKG_VERSION"),
        )
        .is_ok()
    );
    for mutation in 0..10 {
        let mut jobs = simulation_jobs();
        let mut events = triggers(true);
        mutate_simulation(mutation, &mut jobs, &mut events);
        assert!(
            validate_observer_jobs(&jobs, &events, &records, env!("CARGO_PKG_VERSION"),).is_err(),
            "mutation {mutation}"
        );
    }
}

fn mutate_simulation(mutation: u8, jobs: &mut BTreeMap<String, Job>, events: &mut Trigger) {
    match mutation {
        0 => {
            jobs.get_mut(crate::render::PLAN_JOB_ID)
                .expect("Plan")
                .steps[0]
                .condition = Some("true".to_owned())
        }
        1 => {
            let StepKind::Shell { run, .. } = &mut jobs
                .get_mut(crate::render::PLAN_JOB_ID)
                .expect("Plan")
                .steps[0]
                .kind
            else {
                panic!("simulation shell");
            };
            *run = vec!["true".to_owned()];
        }
        2 => jobs
            .get_mut(crate::render::PLAN_JOB_ID)
            .expect("Plan")
            .steps
            .swap(0, 1),
        3 => {
            let simulation = jobs
                .get_mut(crate::render::PLAN_JOB_ID)
                .expect("Plan")
                .steps
                .remove(0);
            jobs.get_mut(crate::render::FINAL_JOB_ID)
                .expect("Required")
                .steps
                .push(simulation);
        }
        4 => {
            events.workflow_dispatch.as_mut().expect("dispatch").inputs[0].input_type =
                DispatchInputType::String
        }
        5 => {
            events.workflow_dispatch.as_mut().expect("dispatch").inputs[0].default =
                Some("true".to_owned())
        }
        6 => {
            jobs.remove("verification-observer");
        }
        7 => {
            jobs.insert(
                "verification-simulation".to_owned(),
                job(vec![simulation_step().expect("simulation")]),
            );
        }
        8 => {
            jobs.get_mut(crate::render::PLAN_JOB_ID)
                .expect("Plan")
                .steps[0]
                .condition = Some(
                "github.event_name == 'workflow_dispatch' && inputs.simulate_failure == true"
                    .to_owned(),
            )
        }
        _ => jobs
            .get_mut(crate::render::FINAL_JOB_ID)
            .expect("Required")
            .needs
            .clear(),
    }
}
