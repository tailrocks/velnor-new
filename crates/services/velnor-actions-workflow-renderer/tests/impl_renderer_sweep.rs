//! Emission sweeps: step names, policy order, candidate check, tree shape.
use velnor_actions_contract_config::{GeneratorValidation, WorkflowPolicy};
use velnor_actions_workflow_jobs::CHECK_GENERATED_NAME;
use velnor_actions_workflow_renderer::{
    render_tree, render_workflow_ir, render_workflow_ir_strict,
};
use velnor_actions_workflow_steps::{RenderError, checkout_step, merge_step, plan_step};
use velnor_actions_workflow_tree::with_marker;

use super::impl_renderer_fixtures::*;

#[test]
fn every_emitted_step_has_name() -> Result<(), RenderError> {
    let mut ctx = fixture_ctx();
    ctx.validator_commands = validator_commands();
    ctx.candidate = Some(velnor_actions_workflow_jobs::CandidateSpec {
        build: mise_argv("mbx@1.0.0", "mbx", &["build"]),
        qualify: vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
    });
    let plan = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            acquire_fixture()?,
            plan_step(),
        ],
    );
    let task = job(
        "velnor-task",
        "Task",
        vec!["plan".to_owned()],
        vec![
            checkout_step(&checkout_pin())?,
            scrubbed_shell_step(
                "Run task",
                vec!["sh".to_owned(), "-c".to_owned(), "echo hi".to_owned()],
            )?,
        ],
    );
    let lint = job(
        "actionlint",
        "Actionlint",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            scrubbed_shell_step(
                "Run actionlint",
                mise_argv("actionlint@1.7.12", "actionlint", &["-color"]),
            )?,
        ],
    );
    let mut final_job = job(
        "required",
        "Required",
        vec!["plan".to_owned()],
        vec![acquire_fixture()?, merge_step()],
    )
    .1;
    final_job.condition = Some("always()".to_owned());
    let ir = fixture_ir(vec![plan, task, lint, ("required".to_owned(), final_job)]);
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Candidate);
    let text = render_workflow_ir_strict(
        &ir,
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &ctx,
        &mise(),
    )?;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("- ") {
            assert!(
                !rest.contains(':') || rest.starts_with("name:"),
                "nameless step item: {line}"
            );
        }
    }
    assert!(text.matches("- name:").count() >= 20, "sweep:\n{text}");
    Ok(())
}

#[test]
fn final_gate_needs_plan_lint_and_support() -> Result<(), RenderError> {
    let mut ctx = fixture_ctx();
    ctx.validator_commands = validator_commands();
    ctx.candidate = Some(velnor_actions_workflow_jobs::CandidateSpec {
        build: mise_argv("mbx@1.0.0", "mbx", &["build"]),
        qualify: vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
    });
    let mut final_job = job(
        "required",
        "Required",
        vec!["plan".to_owned(), "actionlint".to_owned()],
        vec![merge_step()],
    )
    .1;
    final_job.condition = Some("always()".to_owned());
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Candidate);
    let lint = job(
        "actionlint",
        "Actionlint",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?],
    );
    let text = render_workflow_ir(
        &fixture_ir(vec![
            minimal_plan_job()?,
            lint,
            ("required".to_owned(), final_job),
        ]),
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &ctx,
    )?;
    let start = text.find("required:").expect("final job");
    let window = snip(&text, start, 1600);
    for need in [
        "plan",
        "actionlint",
        "alint",
        "cargo-deny",
        "cargo-machete",
        "zizmor",
        "candidate",
    ] {
        assert!(window.contains(need), "missing need {need}:\n{window}");
    }
    assert!(
        !window.contains("release"),
        "release never gates:\n{window}"
    );
    Ok(())
}

#[test]
fn render_carries_no_forbidden_constructs() -> Result<(), RenderError> {
    let ctx = fixture_ctx();
    let text = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?]),
        WorkflowPolicy::ConsumerV1,
        None,
        &ctx,
    )?;
    for absent in [
        "parallel:",
        "background:",
        "wait:",
        " & ",
        "nohup",
        "setsid",
        "disown",
        "run: plan",
        "run: required",
        "run: alint",
        "run: cargo-deny",
        "run: cargo-machete",
        "run: zizmor",
        "run: candidate",
        "pull_request_target",
    ] {
        assert!(!text.contains(absent), "forbidden {absent}:\n{text}");
    }
    Ok(())
}

#[test]
fn default_permissions_block_omits_ungranted_actions() -> Result<(), RenderError> {
    let text = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    let start = text.find("permissions:").expect("permissions");
    let end = text.find("concurrency:").expect("concurrency");
    assert_eq!(&text[start..end], "permissions:\n  contents: read\n");
    Ok(())
}

#[test]
fn validators_render_deny_machete_zizmor_in_order() -> Result<(), RenderError> {
    let mut ctx = fixture_ctx();
    ctx.validator_commands = validator_commands();
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Bootstrap);
    let text = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?]),
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &ctx,
    )?;
    let order = [
        "Checkout",
        "Run cargo-deny",
        "Run cargo-machete",
        "Run zizmor",
    ];
    let mut at = 0;
    for name in order {
        let found = text[at..]
            .find(name)
            .unwrap_or_else(|| panic!("missing {name}:\n{text}"));
        at += found + name.len();
    }
    Ok(())
}

#[test]
fn candidate_check_uses_downloaded_binary() -> Result<(), RenderError> {
    let mut ctx = fixture_ctx();
    ctx.validator_commands = validator_commands();
    ctx.candidate = Some(velnor_actions_workflow_jobs::CandidateSpec {
        build: mise_argv("mbx@1.0.0", "mbx", &["build"]),
        qualify: vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
    });
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Candidate);
    let text = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?]),
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &ctx,
    )?;
    let download = text.find("Download candidate").expect("download");
    let check = text.find(CHECK_GENERATED_NAME).expect("check");
    let qualify = text.find("Qualify candidate").expect("qualify");
    assert!(download < check && check < qualify, "order:\n{text}");
    let job_start = text.find("candidate:").expect("candidate job");
    let job_end = text[job_start..]
        .find("plan:")
        .map_or(text.len(), |at| job_start + at);
    assert_eq!(
        text[job_start..job_end]
            .matches("actions/upload-artifact@")
            .count(),
        1,
        "candidate uploads exactly once:\n{text}"
    );
    // Wide enough to clear the constructor's unset prelude.
    let window = snip(&text, check, 1000);
    assert!(
        window.contains("velnor/candidate/velnor-actions"),
        "{window}"
    );
    assert!(window.contains("generate --output-dir"), "{window}");
    Ok(())
}

#[test]
fn plan_renders_without_task_job() -> Result<(), RenderError> {
    let plan = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, plan_step()],
    );
    let text = render_workflow_ir(
        &fixture_ir(vec![plan]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(text.contains("plan:"), "{text}");
    for absent in ["strategy:", "fromJSON", "velnor-task:"] {
        assert!(!text.contains(absent), "zero-task hit {absent}:\n{text}");
    }
    Ok(())
}

#[test]
fn push_trigger_has_no_path_filters() -> Result<(), RenderError> {
    let plan = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, plan_step()],
    );
    let text = render_workflow_ir(
        &fixture_ir(vec![plan]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    let on_at = text.find("\"on\":").expect("triggers");
    let jobs_at = text.find("jobs:").expect("jobs");
    let window = &text[on_at..jobs_at];
    assert!(window.contains("branches:"), "{window}");
    assert!(!window.contains("paths"), "{window}");
    Ok(())
}

#[test]
fn tree_has_exactly_two_tool_free_files() -> Result<(), RenderError> {
    let plan = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, plan_step()],
    );
    let workflow = render_workflow_ir(
        &fixture_ir(vec![plan]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    let actionlint = with_marker(VERSION, "self-hosted: false\n")?;
    let tree = render_tree(&workflow, &actionlint, VERSION)?;
    assert_eq!(tree.files.len(), 3);
    assert_eq!(tree.symlinks.len(), 1);
    assert_eq!(tree.files[0].path, ".github/AGENTS.md");
    assert_eq!(tree.files[1].path, ".github/actionlint.yaml");
    assert_eq!(tree.files[2].path, ".github/workflows/ci.yml");
    assert_eq!(tree.symlinks[0].path, ".github/CLAUDE.md");
    assert_eq!(tree.symlinks[0].target, "AGENTS.md");
    for file in &tree.files {
        for tool_file in [
            "mise.toml",
            "mise.lock",
            "rust-toolchain.toml",
            ".mise-version",
        ] {
            assert!(
                !file.bytes.contains(tool_file),
                "tool write {} in {}",
                tool_file,
                file.path
            );
        }
    }
    Ok(())
}
