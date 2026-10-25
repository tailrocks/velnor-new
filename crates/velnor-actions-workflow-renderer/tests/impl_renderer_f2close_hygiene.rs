//! F2 closure: cache layers and forbidden content.
use std::collections::BTreeMap;
use velnor_actions_contract::{GeneratorValidation, WorkflowPolicy};
use velnor_actions_workflow_renderer::cache_p08::{ToolsCacheInputs, ToolsCachePayload};
use velnor_actions_workflow_renderer::steps::{TOOLS_RESTORE_ACTION_USES, cache_action_step};
use velnor_actions_workflow_renderer::{
    ALINT_BINARY_VERSION, PUBLISH_PLAN_NAME, RenderError, merge_step, quote_run_arg, quote_scalar,
    render_workflow_ir, shell_step,
};

use super::impl_renderer_fixtures::*;

#[test]
fn cache_action_rejects_empty_paths_and_keys() {
    assert!(
        cache_action_step(true, TOOLS_RESTORE_ACTION_USES, "sources", "k", &[], &[])
            .is_err_and(|err| format!("{err:?}").contains("empty_cache_paths")),
        "empty paths must fail"
    );
    assert!(cache_action_step(true, TOOLS_RESTORE_ACTION_USES, "sources", "", &[], &[]).is_err());
    assert!(
        cache_action_step(
            true,
            TOOLS_RESTORE_ACTION_USES,
            "sources",
            "has space",
            &[],
            &[]
        )
        .is_err()
    );
}

#[test]
fn cache_layers_restore_independently() -> Result<(), RenderError> {
    let sources = cache_action_step(
        true,
        TOOLS_RESTORE_ACTION_USES,
        "sources",
        "k",
        &[],
        &["${{ runner.temp }}/velnor/cargo/registry/cache".to_owned()],
    )?;
    let task = cache_action_step(
        true,
        TOOLS_RESTORE_ACTION_USES,
        "task",
        "k",
        &[],
        &[velnor_actions_workflow_renderer::steps::TASK_ARTIFACTS_DIR.to_owned()],
    )?;
    let specs = ["jactionlint@2.3.0".to_owned()];
    let payload = ToolsCachePayload::new(ToolsCacheInputs {
        runs_on: LABEL,
        target: "x86_64-unknown-linux-gnu",
        mise_setup: &mise(),
        tool_specs: &specs,
        rustup_toolchain: None,
        rustup_components: &[],
        scale_set_image_source_sha: None,
    })?;
    let identity = payload.runtime_prelude_step()?;
    let tools = payload.restore_step()?;
    assert_eq!(
        payload.paths(),
        [
            "~/.local/share/mise",
            "${{ runner.temp }}/velnor/rustup",
            "${{ runner.temp }}/velnor/cargo/.crates.toml",
            "${{ runner.temp }}/velnor/cargo/.crates2.json",
            "${{ runner.temp }}/velnor/cargo/bin",
        ]
    );
    for step in [&sources, &task] {
        let velnor_actions_contract::StepKind::Action { uses, .. } = &step.kind else {
            panic!("restore must be an action step");
        };
        assert!(uses.starts_with("actions/cache/restore@"), "{uses}");
    }
    let velnor_actions_contract::StepKind::Action { uses, .. } = &tools.kind else {
        panic!("tools restore is an action step");
    };
    assert_eq!(
        uses,
        velnor_actions_workflow_renderer::steps::TOOLS_RESTORE_USES
    );
    assert_eq!(identity.name, "V2 identity");
    assert_eq!(
        tools.condition.as_deref(),
        Some(velnor_actions_workflow_renderer::cache_p08::TOOLS_CACHE_RESTORE_CONDITION)
    );
    assert!(
        cache_action_step(
            true,
            TOOLS_RESTORE_ACTION_USES,
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
        "Required verdict",
        vec!["plan".to_owned()],
        vec![acquire_fixture()?, merge_step()],
    )
    .1;
    final_job.condition =
        Some(velnor_actions_workflow_renderer::render::FINAL_CONDITION.to_owned());
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
    for absent in [".alint.yml", "deny.toml", "secrets.", "pull_request_target"] {
        assert!(!text.contains(absent), "forbidden {absent}:\n{text}");
    }
    assert_only_plan_step_token(&text, STAGED)?;
    let remaining = text
        .lines()
        .filter(|line| line.trim() != "GH_TOKEN: ${{ github.token }}")
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        !remaining.contains("github.token"),
        "token expression outside the admitted Plan binding:\n{remaining}"
    );
    // Scrub overlay keys render by construction, except the typed Plan
    // operation's single read-only credential binding.
    for key in ["GITHUB_TOKEN", "ACTIONS_RUNTIME_TOKEN"] {
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

fn assert_only_plan_step_token(text: &str, staged_binary: &str) -> Result<(), RenderError> {
    let mut in_plan_job = false;
    let mut step_lines = Vec::new();
    let mut scoped_tokens = 0_u32;
    let mut nonempty_tokens = 0_u32;

    for line in text.lines() {
        if line.starts_with("  ") && !line.starts_with("    ") {
            check_plan_step_token(&step_lines, staged_binary, &mut scoped_tokens)?;
            step_lines.clear();
            in_plan_job = line.trim() == "plan:";
        } else if in_plan_job && line.trim_start().starts_with("- name: ") {
            check_plan_step_token(&step_lines, staged_binary, &mut scoped_tokens)?;
            step_lines.clear();
            step_lines.push(line.to_owned());
            continue;
        }
        if line.trim_start().starts_with("GH_TOKEN:") {
            let value = line
                .trim_start()
                .strip_prefix("GH_TOKEN:")
                .unwrap_or_default()
                .trim();
            if value != "\"\"" {
                nonempty_tokens += 1;
            }
        }
        if in_plan_job {
            step_lines.push(line.to_owned());
        }
    }
    check_plan_step_token(&step_lines, staged_binary, &mut scoped_tokens)?;
    assert_eq!(nonempty_tokens, 1, "unexpected GH_TOKEN binding:\n{text}");
    assert_eq!(
        scoped_tokens, 1,
        "token must belong to Plan's plan-v1 step:\n{text}"
    );
    Ok(())
}

fn check_plan_step_token(
    lines: &[String],
    staged_binary: &str,
    scoped_tokens: &mut u32,
) -> Result<(), RenderError> {
    let operation = lines
        .iter()
        .any(|line| line.trim() == "VELNOR_INTERNAL_OP: plan-v1");
    let plan_step = lines.iter().any(|line| line.trim() == "- name: Plan");
    let command = quote_run_arg(staged_binary)?;
    let expected_run = format!("run: {}", quote_scalar(&command));
    let staged_sdk_invocation = lines
        .iter()
        .any(|line| line.trim() == expected_run.as_str());
    for line in lines {
        if line.trim_start().starts_with("GH_TOKEN:") {
            assert!(
                plan_step
                    && operation
                    && staged_sdk_invocation
                    && line.trim() == "GH_TOKEN: ${{ github.token }}",
                "GH_TOKEN is only allowed on the Plan plan-v1 step: {lines:#?}"
            );
            *scoped_tokens += 1;
        }
    }
    Ok(())
}

#[test]
fn velnor_policy_renders_with_empty_matrix() -> Result<(), RenderError> {
    let mut ctx = fixture_ctx();
    ctx.validator_commands = validator_commands();
    let mut final_job = job(
        "required",
        "Required verdict",
        vec!["plan".to_owned()],
        vec![merge_step()],
    )
    .1;
    final_job.condition =
        Some(velnor_actions_workflow_renderer::render::FINAL_CONDITION.to_owned());
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
    for id in ["cargo-deny:", "cargo-machete:"] {
        assert!(text.contains(id), "{id}:\n{text}");
    }
    assert!(!text.contains("velnor-task:"), "empty matrix:\n{text}");
    let start = text.find("alint:").expect("alint job");
    let window = snip(&text, start, 1600);
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
    for need in ["plan", "alint", "cargo-deny", "cargo-machete"] {
        assert!(window.contains(need), "missing need {need}:\n{window}");
    }
    for (id, name) in [
        ("cargo-deny:", "Run cargo-deny"),
        ("cargo-machete:", "Run cargo-machete"),
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
    assert_eq!(
        mise.trim(),
        velnor_actions_mise::MISE_VERSION,
        "mise pin drift"
    );
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
