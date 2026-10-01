//! F2 closure: cache layers, forbidden content, token hygiene.
use std::collections::BTreeMap;
use velnor_actions_contract::{GeneratorValidation, NotSelectedReason, WorkflowPolicy};
use velnor_actions_workflow_renderer::steps::{
    TOOLS_RESTORE_USES, cache_action_step, tools_cache_key,
};
use velnor_actions_workflow_renderer::task_steps::{
    NOT_APPLICABLE_REASON, NoOpReport, RESTORE_OBJECTS_NAME, noop_step,
};
use velnor_actions_workflow_renderer::{
    PUBLISH_PLAN_NAME, RenderError, checkout_step, merge_step, plan_step, render_workflow_ir,
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

fn token_plan_job(
    name: &str,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
) -> Result<(String, velnor_actions_contract::Job), RenderError> {
    Ok(job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            shell_step(name, argv, env)?,
            plan_step(),
        ],
    ))
}

fn render_fails_with(jobs: Vec<(String, velnor_actions_contract::Job)>, want: &str) {
    assert!(
        render_workflow_ir(
            &fixture_ir(jobs),
            WorkflowPolicy::ConsumerV1,
            None,
            &fixture_ctx(),
        )
        .is_err_and(|err| format!("{err:?}").contains(want)),
        "must fail with {want}"
    );
}

#[test]
fn token_hygiene_scopes_gh_token_to_plan() -> Result<(), RenderError> {
    use velnor_actions_contract::{Step, StepKind};
    use velnor_actions_workflow_renderer::toolchain_env::with_credential_scrub;
    // The shell constructor rejects `github.token` env values outright:
    // only the render-time fetch binding may carry one.
    let err = shell_step(
        "Plan",
        vec!["true".to_owned()],
        BTreeMap::from([("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned())]),
    )
    .expect_err("github.token in constructor env must fail");
    assert!(
        format!("{err:?}").contains("bad_env_expression"),
        "wrong rejection: {err:?}"
    );
    // Hand-built IR carrying the scoped shape still fails at render.
    let mut scrubbed = with_credential_scrub(&BTreeMap::new());
    scrubbed.insert("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned());
    let scoped = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            Step {
                name: "Plan".to_owned(),
                condition: None,
                kind: StepKind::Shell {
                    run: vec!["true".to_owned()],
                    env: scrubbed,
                },
            },
            plan_step(),
        ],
    );
    render_fails_with(vec![scoped], "bad_env_expression");
    let bad_env = token_plan_job(
        "Leak",
        vec!["true".to_owned()],
        BTreeMap::from([("GITHUB_TOKEN".to_owned(), "x".to_owned())]),
    )?;
    render_fails_with(vec![bad_env], "credential_env");
    Ok(())
}

#[test]
fn token_hygiene_allows_final_fetch_token() -> Result<(), RenderError> {
    use velnor_actions_workflow_renderer::steps::{FETCH_OPERATION, internal_step};
    // The fetch binding is render-time only: the internal op renders
    // its fixed `GH_TOKEN`, never carried through IR env.
    let (id, mut final_job) = job(
        "required",
        "Required",
        vec!["plan".to_owned()],
        vec![
            checkout_step(&checkout_pin())?,
            internal_step("Download every expected matrix artifact", FETCH_OPERATION)?,
            merge_step(),
        ],
    );
    final_job.condition = Some("always()".to_owned());
    let text = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?, (id, final_job)]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(
        text.contains("GH_TOKEN: ${{ github.token }}"),
        "fetch must bind the token:\n{text}"
    );
    Ok(())
}

#[test]
fn token_hygiene_rejects_prints_and_task_tokens() -> Result<(), RenderError> {
    let printed = token_plan_job(
        "Task",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "echo $GH_TOKEN".to_owned(),
        ],
        BTreeMap::new(),
    )?;
    render_fails_with(vec![printed], "token_in_run");
    let task_token = job(
        "velnor-task",
        "Task",
        vec!["plan".to_owned()],
        vec![velnor_actions_contract::Step {
            name: "Run task".to_owned(),
            condition: None,
            kind: velnor_actions_contract::StepKind::Shell {
                run: vec!["true".to_owned()],
                env: BTreeMap::from([("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned())]),
            },
        }],
    );
    render_fails_with(vec![minimal_plan_job()?, task_token], "token_misplaced");
    Ok(())
}

#[test]
fn token_hygiene_allows_empty_scrub_and_rejects_all_nine_keys() -> Result<(), RenderError> {
    use velnor_actions_workflow_renderer::toolchain_env::STEP_CREDENTIAL_DENYLIST;
    let scrub: BTreeMap<String, String> = STEP_CREDENTIAL_DENYLIST
        .iter()
        .map(|key| ((*key).to_owned(), String::new()))
        .collect();
    let scrubbed = job(
        "velnor-task",
        "Task",
        vec!["plan".to_owned()],
        vec![shell_step("Run task", vec!["true".to_owned()], scrub)?],
    );
    render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?, scrubbed]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    for key in STEP_CREDENTIAL_DENYLIST {
        let leaked = job(
            "velnor-task",
            "Task",
            vec!["plan".to_owned()],
            vec![shell_step(
                "Run task",
                vec!["true".to_owned()],
                BTreeMap::from([(key.to_owned(), "x".to_owned())]),
            )?],
        );
        let want = if key == "GH_TOKEN" {
            "token_misplaced"
        } else {
            "credential_env"
        };
        render_fails_with(vec![minimal_plan_job()?, leaked], want);
    }
    Ok(())
}

#[test]
fn scrub_coverage_rejects_bare_and_partial_shell_env() -> Result<(), RenderError> {
    let bare = token_plan_job("Run task", vec!["true".to_owned()], BTreeMap::new())?;
    render_fails_with(vec![bare], "missing_scrub");
    let partial = token_plan_job(
        "Run task",
        vec!["true".to_owned()],
        BTreeMap::from([("GITHUB_TOKEN".to_owned(), String::new())]),
    )?;
    render_fails_with(vec![partial], "missing_scrub");
    Ok(())
}

#[test]
fn scrub_coverage_allows_ambient_auth_steps_and_release() -> Result<(), RenderError> {
    for name in [
        "Prepare pinned tools",
        "Prepare Rust components",
        "Fetch Cargo sources",
    ] {
        let allowed = token_plan_job(name, vec!["true".to_owned()], BTreeMap::new())?;
        render_workflow_ir(
            &fixture_ir(vec![allowed]),
            WorkflowPolicy::ConsumerV1,
            None,
            &fixture_ctx(),
        )?;
    }
    let release = job(
        "release",
        "Release",
        vec!["plan".to_owned()],
        vec![shell_step(
            "Publish release assets",
            vec!["true".to_owned()],
            BTreeMap::new(),
        )?],
    );
    render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?, release]),
        WorkflowPolicy::VelnorRepositoryV1,
        None,
        &fixture_ctx(),
    )?;
    Ok(())
}

#[test]
fn unset_wrapper_passes_but_exfil_behind_it_fails() -> Result<(), RenderError> {
    use velnor_actions_workflow_renderer::toolchain_env::{
        with_credential_unset_script, with_env_unset_argv,
    };
    let scrub_of = |extra: BTreeMap<String, String>| {
        velnor_actions_workflow_renderer::toolchain_env::with_credential_scrub(&extra)
    };
    let wrapped = token_plan_job(
        "Run task",
        with_env_unset_argv(&["mise".to_owned(), "run".to_owned(), "x".to_owned()]),
        scrub_of(BTreeMap::new()),
    )?;
    render_workflow_ir(
        &fixture_ir(vec![wrapped]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    let scripted = token_plan_job(
        "Run task",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            with_credential_unset_script("true"),
        ],
        scrub_of(BTreeMap::new()),
    )?;
    render_workflow_ir(
        &fixture_ir(vec![scripted]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    let smuggled = token_plan_job(
        "Run task",
        with_env_unset_argv(&["echo".to_owned(), "$GH_TOKEN".to_owned()]),
        scrub_of(BTreeMap::new()),
    )?;
    render_fails_with(vec![smuggled], "token_in_run");
    let fake_prelude = token_plan_job(
        "Run task",
        vec![
            "sh".to_owned(),
            "-c".to_owned(),
            "unset FOO; echo $GH_TOKEN".to_owned(),
        ],
        scrub_of(BTreeMap::new()),
    )?;
    render_fails_with(vec![fake_prelude], "token_in_run");
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
    let step = noop_step(RESTORE_OBJECTS_NAME, &report)?;
    let velnor_actions_contract::StepKind::Shell { run, .. } = &step.kind else {
        panic!("no-op must be a shell step");
    };
    for token in ["not_selected", "unsupported", "/tasks/"] {
        assert!(run[2].contains(token), "missing {token}:\n{}", run[2]);
    }
    Ok(())
}
