//! F2 closure: cache layers and forbidden content.
use std::collections::BTreeMap;
use velnor_actions_contract::{GeneratorValidation, Job, WorkflowPolicy};
use velnor_actions_workflow_renderer::cache_p08::{
    ensure_setup_p08, infer_job_tools, mise_cache_key_for_tools,
};
use velnor_actions_workflow_renderer::steps::{TOOLS_RESTORE_USES, cache_action_step};
use velnor_actions_workflow_renderer::{
    ALINT_BINARY_VERSION, PUBLISH_PLAN_NAME, RenderError, merge_step, render_workflow_ir,
    shell_step,
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
fn tools_key_bounded_and_qualified_by_tool_set() -> Result<(), RenderError> {
    let specs = ["rust@1.98.1".to_owned(), "cargo-nextest@0.9.0".to_owned()];
    let key = mise_cache_key_for_tools("x86_64-unknown-linux-gnu", "2026.9.16", &specs)?;
    for part in ["mise-v1", "x86_64-unknown-linux-gnu", "2026.9.16"] {
        assert!(key.contains(part), "missing {part}:\n{key}");
    }
    assert!(!key.contains(' '), "spaces:\n{key}");
    assert_eq!(
        key.len(),
        "mise-v1-x86_64-unknown-linux-gnu-2026.9.16-".len() + 16
    );
    assert_eq!(
        key,
        mise_cache_key_for_tools(
            "x86_64-unknown-linux-gnu",
            "2026.9.16",
            &[specs[1].clone(), specs[0].clone()]
        )?,
        "tool spec ordering does not split the same payload"
    );
    assert_ne!(
        key,
        mise_cache_key_for_tools(
            "x86_64-unknown-linux-gnu",
            "2026.9.16",
            &["rust@1.98.1".to_owned()]
        )?,
        "different payloads use different keys"
    );
    for bad in ["latest", "", "has space"] {
        assert!(
            mise_cache_key_for_tools("x86_64-unknown-linux-gnu", bad, &specs).is_err(),
            "version {bad} must fail"
        );
    }
    assert!(mise_cache_key_for_tools("riscv-none", "2026.9.16", &specs).is_err());
    assert!(mise_cache_key_for_tools("x86_64-unknown-linux-gnu", "2026.9.16", &[]).is_err());
    assert!(
        mise_cache_key_for_tools(
            "x86_64-unknown-linux-gnu",
            "2026.9.16",
            &["rust@latest".to_owned()]
        )
        .is_err()
    );
    for bad in ["gh@latest", "gh@stable", "gh@nightly", "gh@1.2"] {
        assert!(
            mise_cache_key_for_tools("x86_64-unknown-linux-gnu", "2026.9.16", &[bad.to_owned()])
                .is_err(),
            "direct cache keys reject {bad}"
        );
    }
    Ok(())
}

fn job_for_tool_run(run: Vec<String>) -> Job {
    let step = shell_step("Run pinned tools", run, BTreeMap::new()).expect("valid shell step");
    job("cache-test", "Cache test", Vec::new(), vec![step]).1
}

#[test]
fn inferred_latest_tools_fail_closed_instead_of_disappearing_or_bootstrapping() {
    let mixed_run = ["mise", "--no-config", "install", "rust@1.98.1", "gh@latest"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let mut mixed = job_for_tool_run(mixed_run);
    assert_eq!(
        infer_job_tools(&mixed),
        ["gh@latest".to_owned(), "rust@1.98.1".to_owned()]
    );
    assert!(
        ensure_setup_p08(
            "mixed-tools",
            &mut mixed,
            &mise(),
            false,
            "x86_64-unknown-linux-gnu"
        )
        .is_err()
    );

    for always in [false, true] {
        let latest_run = ["mise", "--no-config", "install", "gh@latest"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let mut latest_only = job_for_tool_run(latest_run);
        assert_eq!(infer_job_tools(&latest_only), ["gh@latest".to_owned()]);
        assert!(
            ensure_setup_p08(
                "latest-only",
                &mut latest_only,
                &mise(),
                always,
                "x86_64-unknown-linux-gnu"
            )
            .is_err()
        );
        assert!(
            latest_only
                .steps
                .iter()
                .all(|step| step.name != "Setup Mise"),
            "invalid latest tool must not be replaced by bootstrap"
        );
    }
}

#[test]
fn mise_exec_node_inference_excludes_scoped_child_package_from_cache_key() {
    let run = mise_argv("node@22.19.0", "npm", &["install", "@scope/pkg"]);
    let job = job_for_tool_run(run);
    let tools = infer_job_tools(&job);
    assert_eq!(tools, ["node@22.19.0".to_owned()]);
    assert!(mise_cache_key_for_tools("x86_64-unknown-linux-gnu", "2026.9.16", &tools).is_ok());
}

#[test]
fn cache_layers_restore_independently() -> Result<(), RenderError> {
    let source_paths = [
        ".velnor/cache/cargo/registry/index".to_owned(),
        ".velnor/cache/cargo/registry/cache".to_owned(),
        ".velnor/cache/cargo/git/db".to_owned(),
    ];
    let sources = cache_action_step(true, TOOLS_RESTORE_USES, "sources", "k", &[], &source_paths)?;
    let task = cache_action_step(
        true,
        TOOLS_RESTORE_USES,
        "task",
        "k",
        &[],
        &[velnor_actions_workflow_renderer::steps::TASK_ARTIFACTS_DIR.to_owned()],
    )?;
    let tools_key = mise_cache_key_for_tools(
        "x86_64-unknown-linux-gnu",
        "2026.9.16",
        &["rust@1.98.1".to_owned()],
    )?;
    let tools = velnor_actions_workflow_renderer::steps::tools_restore_step(&tools_key)?;
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
        "pull_request_target",
    ] {
        assert!(!text.contains(absent), "forbidden {absent}:\n{text}");
    }
    // Scrub overlay keys render by construction, but only ever empty.
    for key in ["GH_TOKEN", "GITHUB_TOKEN", "ACTIONS_RUNTIME_TOKEN"] {
        let mut seen = 0_u32;
        for line in text.lines() {
            let Some(rest) = line.trim_start().strip_prefix(key) else {
                continue;
            };
            let rest = rest.strip_prefix(':').unwrap_or(rest).trim();
            assert_eq!(rest, "\"\"", "nonempty {key} binding:\n{text}");
            seen += 1;
        }
        assert!(seen > 0, "missing scrub {key}:\n{text}");
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
    // SHA-pinned `uses:` falls back to `latest` without `version:` (action.yml);
    // the pin value itself is freshness-checked against the reviewed inventory.
    let pinned = format!("version: {ALINT_BINARY_VERSION}");
    assert!(window.contains(&pinned), "missing {pinned}:\n{window}");
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
fn token_hygiene_rejects_any_casing_of_secrets() -> Result<(), RenderError> {
    // Every casing fails closed; only the layer differs. The canonical
    // lowercase binding is constructor-allowlisted, so it must reach
    // the render gate (`token_in_env`); other casings may already fail
    // at construction (`bad_env_expression`).
    let mut render_checked = false;
    for leak in [
        "${{ secrets.CARGO_REGISTRY_TOKEN }}",
        "${{ Secrets.CARGO_REGISTRY_TOKEN }}",
        "${{ SECRETS.CARGO_REGISTRY_TOKEN }}",
    ] {
        let env = BTreeMap::from([("TOKEN_COPY".to_owned(), leak.to_owned())]);
        let argv: Vec<String> = ["mise", "run", "audit"]
            .iter()
            .map(ToString::to_string)
            .collect();
        let leaked = match shell_step("Run audit", argv, env) {
            Ok(step) => step,
            Err(err) => {
                assert!(
                    format!("{err:?}").contains("bad_env_expression"),
                    "wrong rejection: {err:?}"
                );
                continue;
            }
        };
        let task = job("task", "Task", vec!["plan".to_owned()], vec![leaked]);
        let err = render_workflow_ir(
            &fixture_ir(vec![minimal_plan_job()?, task]),
            WorkflowPolicy::ConsumerV1,
            None,
            &fixture_ctx(),
        )
        .expect_err("secrets leak must fail");
        assert!(err.to_string().contains("token_in_env"), "{err}");
        render_checked = true;
    }
    assert!(render_checked, "canonical binding must reach render");
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
