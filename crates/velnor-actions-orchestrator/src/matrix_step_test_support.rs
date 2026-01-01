use super::*;

pub(super) fn generated_pinned_task_step() -> Step {
    let catalog = ToolCatalog::pinned();
    let pinned = PinnedToolExec::new(
        vec![PinnedTool::Rust],
        OsStr::new("cargo"),
        ["test", "-p", "demo"]
            .into_iter()
            .map(OsString::from)
            .collect(),
    )
    .expect("pinned Cargo task");
    let mut task = obligation();
    task.run = pinned
        .argv(&catalog)
        .into_iter()
        .map(|word| word.into_string().expect("UTF-8 pinned task argv"))
        .collect();
    let toolchain = toolchain_id(&task.toolchain_inputs).expect("toolchain identity");
    task.task_digest = task_digest_for_execution(&task.task_id, &task.run, &toolchain)
        .expect("updated task digest");
    obligation_step(&task, &catalog, &[], None, env!("CARGO_PKG_VERSION"))
        .expect("generated task step")
}

pub(super) fn obligation_workflow_ir(checkout_uses: &str, task_step: Step) -> WorkflowIr {
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: ["opened", "synchronize", "reopened", "ready_for_review"]
                .into_iter()
                .map(ToString::to_string)
                .collect(),
            push_branches: vec!["main".to_owned()],
            merge_group: true,
            workflow_dispatch: None,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: velnor_actions_workflow_renderer::CONCURRENCY_GROUP.to_owned(),
            cancel_in_progress: velnor_actions_workflow_renderer::CONCURRENCY_CANCEL.to_owned(),
        },
        jobs: BTreeMap::from([
            ("plan".to_owned(), obligation_plan_job(checkout_uses)),
            (
                "rust-demo".to_owned(),
                obligation_task_job(checkout_uses, task_step),
            ),
        ]),
    }
}

pub(super) fn obligation_plan_job(checkout_uses: &str) -> Job {
    Job {
        check_runner: None,
        display_name: "Plan".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: JobTimeout::PLAN,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![
            checked_out(checkout_uses),
            velnor_actions_workflow_renderer::steps::write_request_step("plan-v1")
                .expect("plan request step"),
            velnor_actions_workflow_renderer::plan_step(),
        ],
    }
}

pub(super) fn obligation_task_job(checkout_uses: &str, task_step: Step) -> Job {
    Job {
        check_runner: None,
        display_name: "Rust / demo".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        timeout_minutes: JobTimeout::CRATE,
        needs: vec!["plan".to_owned()],
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![checked_out(checkout_uses), staged_helper_step(), task_step],
    }
}

pub(super) fn staged_helper_step() -> Step {
    let version = env!("CARGO_PKG_VERSION");
    let staged = format!(
        "{}{}",
        velnor_actions_workflow_renderer::STAGED_BINARY_PREFIX,
        version
    );
    velnor_actions_workflow_renderer::provision_acquire_step(
        &velnor_actions_workflow_renderer::HelperProvenance::ReleaseAsset {
            url: "https://example.invalid/velnor".to_owned(),
            sha256: "a".repeat(64),
            commit: "b".repeat(40),
        },
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!(
                "curl -fsSL \"$VELNOR_ASSET_URL\" -o {staged} && echo \"$VELNOR_ASSET_SHA256  {staged}\" | sha256sum -c - && chmod +x {staged}"
            ),
        ],
    )
    .expect("digest-verified helper staging")
}

pub(super) fn checked_out(checkout_uses: &str) -> Step {
    velnor_actions_workflow_renderer::checkout_step(checkout_uses).expect("checkout step")
}

pub(super) fn renderer_context(
    checkout_uses: String,
) -> velnor_actions_workflow_renderer::render::RenderContext {
    let generator_version = env!("CARGO_PKG_VERSION").to_owned();
    velnor_actions_workflow_renderer::render::RenderContext {
        staged_binary: format!("$RUNNER_TEMP/velnor/bin/velnor-actions-{generator_version}"),
        generator_version: generator_version.clone(),
        report_helper_version: generator_version,
        runs_on: "ubuntu-26.04".to_owned(),
        scale_set_selector: None,
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses,
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        workflow_tasks: Vec::new(),
        pull_request_cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        plan_consumer_env: BTreeMap::new(),
    }
}
