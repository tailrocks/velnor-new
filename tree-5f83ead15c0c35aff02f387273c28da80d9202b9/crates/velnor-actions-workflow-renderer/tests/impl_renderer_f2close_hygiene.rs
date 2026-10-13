//! F2 closure: cache layers and forbidden content.
use std::collections::BTreeMap;
use velnor_actions_contract::{GeneratorValidation, NotSelectedReason, WorkflowPolicy};
use velnor_actions_workflow_renderer::steps::{
    TOOLS_RESTORE_USES, cache_action_step, tools_cache_key,
};
use velnor_actions_workflow_renderer::task_steps::{
    NOOP_REASON_ENV, NOOP_REPORT_OP, NOT_APPLICABLE_REASON, NoOpReport, RESTORE_OBJECTS_NAME,
    noop_step,
};
use velnor_actions_workflow_renderer::{
    PUBLISH_PLAN_NAME, RenderError, merge_step, render_workflow_ir, shell_step,
};

use super::impl_renderer_fixtures::*;
#[test]
fn cache_action_rejects_empty_paths_and_keys() {
    assert!(
        cache_action_step(true, TOOLS_RESTORE_USES, "sources", "k", &[], &[])
            .is_err_and(|err| format!("{err:?}").contains("empty_cache_paths")),
        "empty paths must fail"
    );
    assert!(cache_action_step(true, TOOLS_RESTORE_USES, "sources", "", &[], &[]).is_err());
    assert!(cache_action_step(true, TOOLS_RESTORE_USES, "sources", "has space", &[], &[]).is_err());
}

#[test]
fn tools_key_bounded_and_hashed() -> Result<(), RenderError> {
    let key = tools_cache_key("x86_64-unknown-linux-gnu", "2026.9.16", "0.1.0", "plan")?;
    for part in [
        "mise-tools-v1",
        "x86_64-unknown-linux-gnu",
        "2026.9.16",
        "0.1.0",
        "plan",
        "hashFiles(",
    ] {
        assert!(key.contains(part), "missing {part}:\n{key}");
    }
    assert!(!key.contains(' '), "spaces:\n{key}");
    for bad in ["latest", "", "has space"] {
        assert!(
            tools_cache_key("x86_64-unknown-linux-gnu", bad, "0.1.0", "plan").is_err(),
            "version {bad} must fail"
        );
    }
    assert!(tools_cache_key("riscv-none", "2026.9.16", "0.1.0", "plan").is_err());
    Ok(())
}

#[test]
fn cache_layers_restore_independently() -> Result<(), RenderError> {
    let sources = cache_action_step(
        true,
        TOOLS_RESTORE_USES,
        "sources",
        "k",
        &[],
        &["$CARGO_HOME/registry".to_owned()],
    )?;
    let task = cache_action_step(
        true,
        TOOLS_RESTORE_USES,
        "task",
        "k",
        &[],
        &[velnor_actions_workflow_renderer::steps::TASK_ARTIFACTS_DIR.to_owned()],
    )?;
    let tools = velnor_actions_workflow_renderer::steps::tools_restore_step(&tools_cache_key(
        "x86_64-unknown-linux-gnu",
        "2026.9.16",
        "0.1.0",
        "plan",
    )?)?;
    for step in [&sources, &task, &tools] {
        let velnor_actions_contract::StepKind::Action { uses, .. } = &step.kind else {
            panic!("restore must be an action step");
        };
        assert!(uses.starts_with("actions/cache/restore@"), "{uses}");
    }
    assert!(
        cache_action_step(
            true,
            TOOLS_RESTORE_USES,
            "sources",
            "k",
            &[],
            &[velnor_actions_workflow_renderer::steps::TASK_ARTIFACTS_DIR.to_owned()],
        )
        .is_err(),
        "task path via sources layer must fail"
    );
    Ok(())
}

#[test]
fn render_carries_no_warmup_prune_or_invented_nextest() -> Result<(), RenderError> {
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Candidate);
    let mut final_job = job(
        "required",
        "Required",
        vec!["plan".to_owned()],
        vec![acquire_fixture()?, merge_step()],
    )
    .1;
    final_job.condition = Some("always()".to_owned());
    let text = render_workflow_ir(
        &fixture_ir(vec![
            minimal_plan_job()?,
            matrix_task_job()?,
            ("required".to_owned(), final_job),
        ]),
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &candidate_ctx(),
    )?;
    for absent in ["warmup", "Warmup", "WARMUP", "prune", "Prune", "PRUNE"] {
        assert!(!text.contains(absent), "forbidden {absent}:\n{text}");
    }
    let minimal = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(!minimal.contains("nextest"), "invented:\n{minimal}");
    Ok(())
}

#[test]
fn consumer_render_carries_no_repo_files_or_secrets() -> Result<(), RenderError> {
    let text = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?, matrix_task_job()?]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    for absent in [
        ".alint.yml",
        "deny.toml",
        "secrets.",
        "github.token",
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "ACTIONS_RUNTIME_TOKEN",
        "pull_request_target",
    ] {
        assert!(!text.contains(absent), "forbidden {absent}:\n{text}");
    }
    Ok(())
}

#[test]
fn velnor_policy_renders_with_empty_matrix() -> Result<(), RenderError> {
    let mut ctx = fixture_ctx();
    ctx.validator_commands = validator_commands();
    let mut final_job = job(
        "required",
        "Required",
        vec!["plan".to_owned()],
        vec![merge_step()],
    )
    .1;
    final_job.condition = Some("always()".to_owned());
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Bootstrap);
    let text = render_workflow_ir(
        &fixture_ir(vec![
            minimal_plan_job()?,
            ("required".to_owned(), final_job),
        ]),
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &ctx,
    )?;
    assert!(text.contains("alint:"), "alint:\n{text}");
    for id in ["cargo-deny:", "cargo-machete:", "zizmor:"] {
        assert!(text.contains(id), "{id}:\n{text}");
    }
    assert!(!text.contains("velnor-task:"), "empty matrix:\n{text}");
    let start = text.find("alint:").expect("alint job");
    let window = snip(&text, start, 800);
    for input in [
        "path: .",
        "config: .alint.yml",
        "format: github",
        "fail-on-warning: \"true\"",
    ] {
        assert!(window.contains(input), "missing {input}:\n{window}");
    }
    let start = text.find("required:").expect("final job");
    let window = snip(&text, start, 600);
    for need in ["plan", "alint", "cargo-deny", "cargo-machete", "zizmor"] {
        assert!(window.contains(need), "missing need {need}:\n{window}");
    }
    for (id, name) in [
        ("cargo-deny:", "Run cargo-deny"),
        ("cargo-machete:", "Run cargo-machete"),
        ("zizmor:", "Run zizmor"),
    ] {
        let start = text.find(id).unwrap_or_else(|| panic!("{id} job:\n{text}"));
        let window = snip(&text, start, 900);
        assert!(window.contains(name), "missing {name}:\n{window}");
    }
    assert!(text.contains(PUBLISH_PLAN_NAME), "publish:\n{text}");
    Ok(())
}

#[test]
fn repo_config_sets_velnor_repository_v1() -> Result<(), String> {
    let path = format!("{}/../../.velnor/config.toml", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).map_err(|err| format!("config:{err}"))?;
    let policy = text
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("policy ="))
        .ok_or_else(|| "policy line present".to_owned())?;
    assert!(
        policy.contains("velnor-repository-v1"),
        "policy line: {policy}"
    );
    let pinned = format!("{}/../../.mise-version", env!("CARGO_MANIFEST_DIR"));
    let mise = std::fs::read_to_string(&pinned).map_err(|err| format!("mise-version:{err}"))?;
    assert_eq!(mise.trim(), "2026.9.18", "mise pin drift");
    Ok(())
}

#[test]
fn not_applicable_maps_to_unsupported_report() -> Result<(), RenderError> {
    assert_eq!(NOT_APPLICABLE_REASON, NotSelectedReason::Unsupported);
    let report = NoOpReport {
        task_id: "stack/rust/crates/velnor-actions-contract/clippy/default".to_owned(),
        task_digest: format!("b3-{}", "a".repeat(64)),
        reason: NOT_APPLICABLE_REASON,
    };
    let step = noop_step(
        RESTORE_OBJECTS_NAME,
        &report,
        "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.0",
    )?;
    let velnor_actions_contract::StepKind::Shell { run, .. } = &step.kind else {
        panic!("no-op must be a shell step");
    };
    for token in [NOOP_REPORT_OP, NOOP_REASON_ENV, "unsupported"] {
        assert!(run[2].contains(token), "missing {token}:\n{}", run[2]);
    }
    for forbidden in ["not_selected", "/tasks/", "printf"] {
        assert!(
            !run[2].contains(forbidden),
            "report bytes come from Rust: {forbidden}:\n{}",
            run[2]
        );
    }
    Ok(())
}

#[test]
fn token_hygiene_rejects_any_casing_of_secrets() -> Result<(), RenderError> {
    for leak in [
        "${{ secrets.CARGO_REGISTRY_TOKEN }}",
        "${{ Secrets.CARGO_REGISTRY_TOKEN }}",
        "${{ SECRETS.CARGO_REGISTRY_TOKEN }}",
    ] {
        let env = BTreeMap::from([("TOKEN_COPY".to_owned(), leak.to_owned())]);
        let leaked = shell_step(
            "Run audit",
            ["mise", "run", "audit"]
                .iter()
                .map(ToString::to_string)
                .collect(),
            env,
        )?;
        let task = job("task", "Task", vec!["plan".to_owned()], vec![leaked]);
        let err = render_workflow_ir(
            &fixture_ir(vec![minimal_plan_job()?, task]),
            WorkflowPolicy::ConsumerV1,
            None,
            &fixture_ctx(),
        )
        .expect_err("secrets leak must fail");
        assert!(err.to_string().contains("token_in_env"), "{err}");
    }
    let run = shell_step(
        "Run audit",
        ["echo", "${{ secrets.TOKEN }}"]
            .iter()
            .map(ToString::to_string)
            .collect(),
        BTreeMap::new(),
    )?;
    let task = job("task", "Task", vec!["plan".to_owned()], vec![run]);
    let err = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?, task]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )
    .expect_err("run leak must fail");
    assert!(err.to_string().contains("token_in_run"), "{err}");
    Ok(())
}
