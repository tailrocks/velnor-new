use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_actionlint::{PinnedActionRef, actions};
use velnor_actions_contract::{
    CacheMode, Job, JobTimeout, REQUIRED_CONDITION, REQUIRED_DISPLAY_NAME, SCALE_SET_NAME,
    ScaleSetSelector, Step, StepKind, VELNOR_LABEL, WorkflowIr,
    cachekey::{ToolchainInputs, toolchain_id},
    workflow::{TaskBindingDigest, crate_job::task_digest_for_execution},
};

use crate::{
    RenderedFile,
    lane_share::job_workflow_extract::{
        JobWorkflowRelocation, PROTOTYPE_JOB_ID, PROTOTYPE_LOCAL_JOB_ID,
    },
    lane_share::share_lanes,
    render::{FINAL_JOB_ID, PLAN_JOB_ID, RenderContext, WORKFLOW_PATH, fallback},
    tree,
    workflow_routing::{WorkflowRoutingPolicy, route_workflow_yaml_for_jobs},
    yaml::Yaml,
};

pub(super) const LOCAL_JOB_ID: &str = PROTOTYPE_LOCAL_JOB_ID;

pub(super) struct RenderedFixture {
    pub(super) workflow: WorkflowIr,
    pub(super) policy: WorkflowRoutingPolicy,
    pub(super) parent: Yaml,
    pub(super) files: Vec<RenderedFile>,
    pub(super) relocations: Vec<JobWorkflowRelocation>,
    pub(super) source_task_digest: String,
}

pub(super) fn rendered_fixture() -> RenderedFixture {
    let selector = ScaleSetSelector::try_new(
        SCALE_SET_NAME,
        &[VELNOR_LABEL.to_owned(), SCALE_SET_NAME.to_owned()],
    )
    .expect("fixture Scale Set selector is valid");
    let mut context = context();
    context.scale_set_selector = Some(selector.clone());

    let source_jobs = source_jobs(&context, &selector);
    let source_ir = super::workflow_with_jobs(source_jobs.clone());
    source_ir
        .validate()
        .expect("typed plan, paired jobs, and Required graph are valid");
    let StepKind::TaskExecution { task_digest, .. } = &source_jobs[PROTOTYPE_JOB_ID].steps[3].kind
    else {
        panic!("the source CLI check is a typed task before production lowering");
    };
    let source_task_digest = task_digest.as_str().to_owned();
    let (jobs, task_files) = crate::task_wrapper::factor_obligation_steps(
        &source_jobs,
        &context.checkout_uses,
        &context.generator_version,
        &context.report_helper_version,
        &context.workflow_tasks,
        Some(&selector),
    )
    .expect("the typed CLI task is lowered through the production task builder");
    let workflow = super::workflow_with_jobs(jobs.clone());
    workflow
        .validate()
        .expect("lowered workflow preserves a valid job graph");
    let shared = share_lanes(&jobs, &context).expect("paired CLI lanes share safely");
    assert!(shared.job_workflows.contains_key(PROTOTYPE_JOB_ID));

    let parent = crate::document::workflow_to_yaml(&workflow, &shared, &context, &BTreeSet::new())
        .expect("parent caller serializes from the typed job");
    let policy = crate::workflow_routing::test_policy(
        velnor_actions_contract::ExecutionMode::Both,
        selector,
    );
    let parent = route_workflow_yaml_for_jobs(WORKFLOW_PATH, parent, &policy, &shared.jobs)
        .expect("Both mode retains hosted and local jobs");
    let parent_bytes =
        fallback::render_checked_workflow(WORKFLOW_PATH, &parent, &context.generator_version)
            .expect("the normal checked workflow serializer emits ci.yml");
    let relocations = crate::document::render_job_workflow_relocations_with_routing(
        &workflow,
        &shared,
        &context,
        &BTreeSet::new(),
        &policy,
    )
    .expect("the normal child serializer emits the source-derived callee");

    let mut files = task_files;
    files.extend(shared.files);
    files.extend(
        relocations
            .iter()
            .map(|relocation| relocation.expected_callee_file.clone()),
    );
    files.push(tree::RenderedFile {
        path: WORKFLOW_PATH.to_owned(),
        bytes: parent_bytes,
    });
    files.sort_by(|left, right| left.path.cmp(&right.path));

    RenderedFixture {
        workflow,
        policy,
        parent,
        files,
        relocations,
        source_task_digest,
    }
}

fn context() -> RenderContext {
    RenderContext {
        tool_pins: Vec::new(),
        generator_version: "0.1.0".to_owned(),
        report_helper_version: "0.1.0".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        scale_set_selector: None,
        scale_set_image_source_sha: None,
        verified_cache_helpers: BTreeMap::new(),
        require_verified_cache_helper: false,
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0".to_owned(),
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: format!("actions/checkout@{:040x}", 0),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        workflow_tasks: Vec::new(),
        pull_request_cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        mise_quiet: true,
        plan_consumer_env: BTreeMap::new(),
    }
}

fn source_jobs(context: &RenderContext, selector: &ScaleSetSelector) -> BTreeMap<String, Job> {
    let mut jobs = BTreeMap::from([(
        PLAN_JOB_ID.to_owned(),
        job(
            "Plan",
            "ubuntu-26.04",
            Vec::new(),
            None,
            vec![crate::steps::plan_step()],
            None,
        ),
    )]);
    let mut steps = vec![
        crate::steps::checkout_step(&context.checkout_uses).expect("pinned checkout"),
        acquire_step(context.report_helper_version.as_str()),
    ];
    let (restore, save) = source_cache_steps();
    steps.push(restore);
    steps.push(task_step(context.report_helper_version.as_str()));
    steps.push(parallel_step("Parallel cargo metadata check"));
    steps.push(parallel_step("Parallel format check"));
    steps.push(save);
    jobs.insert(
        PROTOTYPE_JOB_ID.to_owned(),
        cli_job(
            "Rust CLI hosted",
            "ubuntu-26.04",
            PROTOTYPE_JOB_ID,
            steps.clone(),
        ),
    );
    jobs.insert(
        PROTOTYPE_LOCAL_JOB_ID.to_owned(),
        cli_job(
            "Rust CLI local",
            &selector.token(),
            PROTOTYPE_LOCAL_JOB_ID,
            steps,
        ),
    );
    let required = job(
        REQUIRED_DISPLAY_NAME,
        "ubuntu-26.04",
        vec![
            PLAN_JOB_ID.to_owned(),
            PROTOTYPE_JOB_ID.to_owned(),
            PROTOTYPE_LOCAL_JOB_ID.to_owned(),
        ],
        Some(REQUIRED_CONDITION.to_owned()),
        vec![crate::steps::merge_step()],
        None,
    );
    jobs.insert(FINAL_JOB_ID.to_owned(), required);
    crate::required_gate::insert_required_gate(&mut jobs)
        .expect("the production Required gate builder accepts the verdict job");
    jobs
}

fn cli_job(display_name: &str, runs_on: &str, id: &str, steps: Vec<Step>) -> Job {
    let condition = format!(
        "${{{{ needs.plan.result == 'success' && !contains(fromJSON(needs.plan.outputs.not_affected_jobs), '{id}') }}}}"
    );
    job(
        display_name,
        runs_on,
        vec![PLAN_JOB_ID.to_owned()],
        Some(condition),
        steps,
        Some(CacheMode::push_write_expression()),
    )
}

fn job(
    display_name: &str,
    runs_on: &str,
    needs: Vec<String>,
    condition: Option<String>,
    steps: Vec<Step>,
    cache_mode: Option<CacheMode>,
) -> Job {
    Job {
        display_name: display_name.to_owned(),
        runs_on: runs_on.to_owned(),
        check_runner: None,
        timeout_minutes: if display_name == "Plan" {
            JobTimeout::PLAN
        } else if display_name == REQUIRED_DISPLAY_NAME {
            JobTimeout::REQUIRED
        } else {
            JobTimeout::CRATE
        },
        needs,
        condition,
        permissions: None,
        cache_mode,
        environment: None,
        services: Vec::new(),
        steps,
    }
}

fn acquire_step(version: &str) -> Step {
    let helper = format!("{}{version}", crate::steps::STAGED_BINARY_PREFIX);
    crate::steps::acquire_velnor_step(
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!(
                "curl -fsSL \"$VELNOR_ASSET_URL\" -o {helper} && echo \"$VELNOR_ASSET_SHA256  {helper}\" | sha256sum -c - && chmod +x {helper}"
            ),
        ],
        &BTreeMap::from([
            (
                crate::steps::ASSET_URL_ENV.to_owned(),
                "https://example.invalid/velnor".to_owned(),
            ),
            (crate::steps::ASSET_SHA_ENV.to_owned(), "a".repeat(64)),
            (crate::steps::RELEASE_COMMIT_ENV.to_owned(), "b".repeat(40)),
        ]),
    )
    .expect("the pinned helper staging step validates")
}

fn source_cache_steps() -> (Step, Step) {
    let paths = [
        "${{ runner.temp }}/velnor/cargo/registry/index",
        "${{ runner.temp }}/velnor/cargo/registry/cache",
        "${{ runner.temp }}/velnor/cargo/git/db",
    ]
    .map(str::to_owned);
    // The cache-step constructor permits these allowlisted expressions when
    // the surrounding cache key has no whitespace. Keep the exact plan
    // output references so relocation must bind both values as workflow inputs.
    let key = "${{needs.plan.outputs.sources_key}}";
    let restore_keys = ["${{needs.plan.outputs.sources_restore_keys}}".to_owned()];
    let restore_uses = PinnedActionRef::new(
        "actions/cache",
        Some("restore"),
        actions::CACHE_ACTION_SHA,
        actions::CACHE_ACTION_VERSION,
    )
    .expect("pinned sources-cache restore");
    let mut restore = crate::steps::cache_action_step(
        true,
        &restore_uses.uses_value(),
        "sources",
        key,
        &restore_keys,
        &paths,
    )
    .expect("source cache restore step");
    restore.condition = Some("needs.plan.result == 'success'".to_owned());
    let save_uses = PinnedActionRef::new(
        "actions/cache",
        Some("save"),
        actions::CACHE_ACTION_SHA,
        actions::CACHE_ACTION_VERSION,
    )
    .expect("pinned sources-cache save");
    let mut save = crate::steps::cache_action_step(
        false,
        &save_uses.uses_value(),
        "sources",
        key,
        &[],
        &paths,
    )
    .expect("source cache save step");
    save.condition = Some(velnor_actions_contract::workflow::ir::CACHE_SAVE_CONDITION.to_owned());
    (restore, save)
}

fn task_step(report_helper_version: &str) -> Step {
    let task_id = "stack/rust/velnor-actions-cli/test/default";
    let toolchain_inputs = ToolchainInputs {
        tools: vec!["rust@1.99.0".to_owned()],
        components: vec!["clippy".to_owned(), "rustfmt".to_owned()],
        compile_driver: "cargo".to_owned(),
        test_runner: "cargo_test".to_owned(),
    };
    let argv = [
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
        "rust@1.99.0",
        "--",
        "cargo",
        "test",
        "-p",
        "velnor-actions-cli",
    ]
    .map(str::to_owned)
    .to_vec();
    let digest = task_digest_for_execution(
        task_id,
        &argv,
        &toolchain_id(&toolchain_inputs).expect("toolchain identity"),
    )
    .expect("task digest");
    let matrix_id = velnor_actions_contract::matrix_id_for_task_group("rust", task_id)
        .expect("matrix identity");
    let matrix_key = velnor_actions_contract::matrix_key_for_id(&matrix_id).expect("matrix key");
    let env = BTreeMap::from([
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        ("MISE_NO_ENV".to_owned(), "1".to_owned()),
        ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
        ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
        ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
        (
            "MISE_RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        (
            "MISE_CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
        (
            "RUSTUP_HOME".to_owned(),
            "${{ runner.temp }}/velnor/rustup".to_owned(),
        ),
        (
            "CARGO_HOME".to_owned(),
            "${{ runner.temp }}/velnor/cargo".to_owned(),
        ),
        ("RUSTUP_TOOLCHAIN".to_owned(), "1.99.0".to_owned()),
    ]);
    Step {
        parallel: false,
        name: "Run the typed CLI test task".to_owned(),
        id: None,
        role: None,
        condition: None,
        kind: StepKind::TaskExecution {
            argv,
            env,
            task_id: task_id.to_owned(),
            task_digest: TaskBindingDigest::new(digest).expect("task binding digest"),
            toolchain_inputs,
            matrix_id,
            matrix_key,
            report_helper_version: report_helper_version.to_owned(),
            matrix_max_parallel: None,
        },
    }
}

fn parallel_step(name: &str) -> Step {
    let mut step = crate::steps::shell_step(name, vec!["true".to_owned()], BTreeMap::new())
        .expect("parallel check step");
    step.parallel = true;
    step
}
