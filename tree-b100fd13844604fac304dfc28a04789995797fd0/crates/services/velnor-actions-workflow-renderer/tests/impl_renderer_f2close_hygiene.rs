//! F2 closure: cache layers and forbidden content.
use std::collections::BTreeMap;
use velnor_actions_contract_config::{GeneratorValidation, WorkflowPolicy};
use velnor_actions_workflow_renderer::cache_steps::{
    TOOLS_RESTORE_USES, cache_action_step, tools_cache_key,
};
use velnor_actions_workflow_renderer::{PUBLISH_PLAN_NAME, render_workflow_ir};
use velnor_actions_workflow_steps::{ALINT_BINARY_VERSION, RenderError, merge_step, shell_step};

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
        &[velnor_actions_workflow_renderer::cache_steps::TASK_ARTIFACTS_DIR.to_owned()],
    )?;
    let tools = velnor_actions_workflow_renderer::cache_steps::tools_restore_step(
        &tools_cache_key("x86_64-unknown-linux-gnu", "2026.9.16", "0.1.0", "plan")?,
    )?;
    for step in [&sources, &task, &tools] {
        let velnor_actions_contract_workflow::StepKind::Action { uses, .. } = &step.kind else {
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
            &[velnor_actions_workflow_renderer::cache_steps::TASK_ARTIFACTS_DIR.to_owned()],
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
    let run_at = text[start..].find("Run Alint").expect("alint run step") + start;
    let window = snip(&text, run_at, 1600);
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
    let window = snip(&text, start, 1600);
    for need in ["plan", "alint", "cargo-deny", "cargo-machete", "zizmor"] {
        assert!(window.contains(need), "missing need {need}:\n{window}");
    }
    for (id, name) in [
        ("cargo-deny:", "Run cargo-deny"),
        ("cargo-machete:", "Run cargo-machete"),
        ("zizmor:", "Run zizmor"),
    ] {
        let start = text.find(id).unwrap_or_else(|| panic!("{id} job:\n{text}"));
        let window = snip(&text, start, 1600);
        assert!(window.contains(name), "missing {name}:\n{window}");
    }
    assert!(text.contains(PUBLISH_PLAN_NAME), "publish:\n{text}");
    Ok(())
}

#[test]
fn repo_config_sets_velnor_repository_v1() -> Result<(), String> {
    let path = format!(
        "{}/../../../.velnor/config.toml",
        env!("CARGO_MANIFEST_DIR")
    );
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
    let pinned = format!("{}/../../../.mise-version", env!("CARGO_MANIFEST_DIR"));
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
