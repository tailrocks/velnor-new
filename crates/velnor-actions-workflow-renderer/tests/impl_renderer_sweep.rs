//! Emission sweeps: step names, policy order, candidate check, tree shape.
use std::collections::BTreeMap;
use velnor_actions_contract::{GeneratorValidation, WorkflowPolicy};
use velnor_actions_workflow_renderer::{
    CHECK_GENERATED_NAME, PolicyCommand, RenderError, checkout_step, merge_step, plan_step,
    render_tree, render_workflow_ir, render_workflow_ir_strict, shell_step, with_marker,
};

use super::impl_renderer_fixtures::*;

#[test]
fn every_emitted_step_has_name() -> Result<(), RenderError> {
    let mut ctx = fixture_ctx();
    ctx.policy_commands = vec![PolicyCommand {
        name: "Verify pinned tools".to_owned(),
        argv: mise_argv("gh@2.0.0", "gh", &["--version"]),
    }];
    ctx.candidate = Some(velnor_actions_workflow_renderer::CandidateSpec {
        build: mise_argv("mbx@1.0.0", "mbx", &["build"]),
        qualify: vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
    });
    let plan = job(
        "velnor-plan",
        "Velnor Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            acquire_fixture()?,
            plan_step(),
        ],
    );
    let task = job(
        "velnor-task",
        "Velnor Task",
        vec!["velnor-plan".to_owned()],
        vec![
            checkout_step(&checkout_pin())?,
            shell_step(
                "Run task",
                vec!["sh".to_owned(), "-c".to_owned(), "echo hi".to_owned()],
                BTreeMap::new(),
            )?,
        ],
    );
    let lint = job(
        "velnor-workflow-lint",
        "Velnor Workflow Lint",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            shell_step(
                "Run actionlint",
                mise_argv("actionlint@1.7.12", "actionlint", &["-color"]),
                BTreeMap::new(),
            )?,
        ],
    );
    let mut final_job = job(
        "velnor-final",
        "Velnor / Required",
        vec!["velnor-plan".to_owned()],
        vec![acquire_fixture()?, merge_step()],
    )
    .1;
    final_job.condition = Some("always()".to_owned());
    let ir = fixture_ir(vec![
        plan,
        task,
        lint,
        ("velnor-final".to_owned(), final_job),
    ]);
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
fn final_gate_needs_plan_task_lint_and_support() -> Result<(), RenderError> {
    let mut ctx = fixture_ctx();
    ctx.policy_commands = vec![PolicyCommand {
        name: "Verify pinned tools".to_owned(),
        argv: vec!["true".to_owned()],
    }];
    ctx.candidate = Some(velnor_actions_workflow_renderer::CandidateSpec {
        build: mise_argv("mbx@1.0.0", "mbx", &["build"]),
        qualify: vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
    });
    let mut final_job = job(
        "velnor-final",
        "Velnor / Required",
        vec!["velnor-plan".to_owned(), "velnor-workflow-lint".to_owned()],
        vec![merge_step()],
    )
    .1;
    final_job.condition = Some("always()".to_owned());
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Candidate);
    let lint = job(
        "velnor-workflow-lint",
        "Velnor Workflow Lint",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?],
    );
    let text = render_workflow_ir(
        &fixture_ir(vec![
            minimal_plan_job()?,
            lint,
            ("velnor-final".to_owned(), final_job),
        ]),
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &ctx,
    )?;
    let start = text.find("velnor-final:").expect("final job");
    let window = snip(&text, start, 600);
    for need in [
        "velnor-plan",
        "velnor-workflow-lint",
        "velnor-alint",
        "velnor-policy",
        "velnor-candidate",
    ] {
        assert!(window.contains(need), "missing need {need}:\n{window}");
    }
    assert!(
        !window.contains("velnor-release"),
        "release never gates:\n{window}"
    );
    Ok(())
}

#[test]
fn render_carries_no_forbidden_constructs() -> Result<(), RenderError> {
    let mut ctx = fixture_ctx();
    ctx.policy_commands = vec![PolicyCommand {
        name: "Verify pinned tools".to_owned(),
        argv: vec!["true".to_owned()],
    }];
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
        "run: velnor-plan",
        "run: velnor-task",
        "run: velnor-final",
        "run: velnor-policy",
        "run: velnor-alint",
        "run: velnor-candidate",
        "pull_request_target",
    ] {
        assert!(!text.contains(absent), "forbidden {absent}:\n{text}");
    }
    Ok(())
}

#[test]
fn permissions_block_is_exactly_read_read() -> Result<(), RenderError> {
    let text = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    let start = text.find("permissions:").expect("permissions");
    let end = text.find("concurrency:").expect("concurrency");
    assert_eq!(
        &text[start..end],
        "permissions:\n  contents: read\n  actions: read\n"
    );
    Ok(())
}

#[test]
fn policy_renders_deny_machete_zizmor_in_order() -> Result<(), RenderError> {
    let mut ctx = fixture_ctx();
    ctx.policy_commands = vec![
        PolicyCommand {
            name: "Run cargo-deny".to_owned(),
            argv: mise_argv("cargo-deny@0.18.0", "cargo", &["deny", "check"]),
        },
        PolicyCommand {
            name: "Run cargo-machete".to_owned(),
            argv: mise_argv("cargo-machete@0.8.0", "cargo", &["machete"]),
        },
        PolicyCommand {
            name: "Run zizmor".to_owned(),
            argv: mise_argv("zizmor@1.0.0", "zizmor", &["--no-online-audits"]),
        },
    ];
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
    ctx.policy_commands = vec![PolicyCommand {
        name: "Verify pinned tools".to_owned(),
        argv: vec!["true".to_owned()],
    }];
    ctx.candidate = Some(velnor_actions_workflow_renderer::CandidateSpec {
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
    let job_start = text.find("velnor-candidate:").expect("candidate job");
    let job_end = text[job_start..]
        .find("velnor-plan:")
        .map_or(text.len(), |at| job_start + at);
    assert_eq!(
        text[job_start..job_end]
            .matches("actions/upload-artifact@")
            .count(),
        1,
        "candidate uploads exactly once:\n{text}"
    );
    let window = snip(&text, check, 600);
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
        "velnor-plan",
        "Velnor Plan",
        Vec::new(),
        vec![checkout_step(&checkout_pin())?, plan_step()],
    );
    let text = render_workflow_ir(
        &fixture_ir(vec![plan]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(text.contains("velnor-plan:"), "{text}");
    for absent in ["strategy:", "fromJSON", "velnor-task:"] {
        assert!(!text.contains(absent), "zero-task hit {absent}:\n{text}");
    }
    Ok(())
}

#[test]
fn push_trigger_has_no_path_filters() -> Result<(), RenderError> {
    let plan = job(
        "velnor-plan",
        "Velnor Plan",
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
        "velnor-plan",
        "Velnor Plan",
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
    assert_eq!(tree.files.len(), 2);
    assert_eq!(tree.files[0].path, ".github/actionlint.yaml");
    assert_eq!(tree.files[1].path, ".github/workflows/velnor.yml");
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
