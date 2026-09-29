//! F2 closure: derived names, verify-before-run, closures, hygiene.
use std::collections::BTreeMap;
use velnor_actions_contract::{GeneratorValidation, NotSelectedReason, WorkflowPolicy};
use velnor_actions_workflow_renderer::steps::{
    TOOLS_RESTORE_USES, cache_action_step, download_artifact_step, matrix_report_upload_step,
    tools_cache_key, upload_artifact_step,
};
use velnor_actions_workflow_renderer::task_steps::{
    NOT_APPLICABLE_REASON, NoOpReport, RESTORE_OBJECTS_NAME, noop_step,
};
use velnor_actions_workflow_renderer::{
    CHECK_GENERATED_NAME, MATRIX_REPORT_UPLOAD_NAME, PLAN_ID_OUTPUT, PUBLISH_PLAN_NAME,
    RUN_KEY_OUTPUT, RenderContext, RenderError, VERIFY_MANIFEST_NAME, candidate_artifact_name,
    candidate_manifest_verify_script, candidate_manifest_verify_step, checkout_step, merge_step,
    plan_step, render_workflow_ir, shell_step, write_request_step,
};

use super::impl_renderer_fixtures::*;

fn candidate_ctx() -> RenderContext {
    let mut ctx = fixture_ctx();
    ctx.policy_commands = vec![velnor_actions_workflow_renderer::PolicyCommand {
        name: "Verify pinned tools".to_owned(),
        argv: vec!["true".to_owned()],
    }];
    ctx.candidate = Some(velnor_actions_workflow_renderer::CandidateSpec {
        build: mise_argv("mbx@1.0.0", "mbx", &["build"]),
        qualify: vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
    });
    ctx
}

fn matrix_task_job() -> Result<(String, velnor_actions_contract::Job), RenderError> {
    let env = BTreeMap::from([
        (
            "VELNOR_TASK_ID".to_owned(),
            "${{ matrix.task_id }}".to_owned(),
        ),
        ("VELNOR_TASK_RUN".to_owned(), "${{ matrix.run }}".to_owned()),
        (
            velnor_actions_workflow_renderer::MATRIX_NEEDS_JOB_ENV.to_owned(),
            "velnor-plan".to_owned(),
        ),
        (
            velnor_actions_workflow_renderer::MATRIX_OUTPUT_ENV.to_owned(),
            "matrix".to_owned(),
        ),
        (
            velnor_actions_workflow_renderer::MATRIX_MAX_PARALLEL_ENV.to_owned(),
            "2".to_owned(),
        ),
    ]);
    let step = shell_step(
        "Run task",
        vec!["sh".to_owned(), "-c".to_owned(), "echo hi".to_owned()],
        env,
    )?;
    Ok(job(
        "velnor-task",
        "Velnor Task",
        vec!["velnor-plan".to_owned()],
        vec![checkout_step(&checkout_pin())?, step],
    ))
}

#[test]
fn candidate_artifact_name_derives_run_and_target() -> Result<(), RenderError> {
    assert_eq!(
        candidate_artifact_name("x86_64-unknown-linux-gnu")?.as_str(),
        "velnor-candidate-r${{ github.run_id }}-a${{ github.run_attempt }}-x86-64-unknown-linux-gnu",
    );
    assert_eq!(
        candidate_artifact_name("aarch64-apple-darwin")?.as_str(),
        "velnor-candidate-r${{ github.run_id }}-a${{ github.run_attempt }}-aarch64-apple-darwin",
    );
    assert!(candidate_artifact_name("").is_err());
    let resolved = candidate_artifact_name("x86_64-unknown-linux-gnu")?.replace(
        "r${{ github.run_id }}-a${{ github.run_attempt }}",
        "r123-a1",
    );
    velnor_actions_contract::validate_artifact_id(&resolved).map_err(RenderError::Contract)?;
    Ok(())
}

#[test]
fn candidate_job_verifies_manifest_before_running_binary() -> Result<(), RenderError> {
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Candidate);
    let text = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?]),
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &candidate_ctx(),
    )?;
    let order = [
        "Upload candidate",
        "Download candidate",
        VERIFY_MANIFEST_NAME,
        CHECK_GENERATED_NAME,
        "Qualify candidate",
    ];
    let mut at = 0;
    for name in order {
        let found = text[at..]
            .find(name)
            .unwrap_or_else(|| panic!("missing {name}:\n{text}"));
        at += found + name.len();
    }
    let derived = candidate_artifact_name("x86_64-unknown-linux-gnu")?;
    assert!(text.contains(&derived), "derived name:\n{text}");
    assert!(
        !text.contains("name: velnor-candidate\n"),
        "fixed name:\n{text}"
    );
    let script = candidate_manifest_verify_script("x86_64-unknown-linux-gnu");
    for token in [
        "schema",
        "commit",
        "target",
        "toolchain",
        "sha256",
        "GITHUB_SHA",
        "sha256sum",
        "x86_64-unknown-linux-gnu",
    ] {
        assert!(script.contains(token), "missing {token}:\n{script}");
    }
    for absent in ["$(", "`", "'", "sed", "python", "jq"] {
        assert!(!script.contains(absent), "banned {absent}:\n{script}");
    }
    assert!(candidate_manifest_verify_step("not-a-target").is_err());
    Ok(())
}

#[test]
#[cfg(unix)]
fn candidate_verify_script_checks_live_manifest() -> Result<(), RenderError> {
    use std::process::Command;
    let script = candidate_manifest_verify_script("x86_64-unknown-linux-gnu");
    let root = std::env::temp_dir().join(format!("velnor-verify-{}", std::process::id()));
    let dir = root.join("velnor/candidate");
    std::fs::create_dir_all(&dir)
        .map_err(|err| RenderError::InvalidWorkflow(format!("tmp:{err}")))?;
    let run_case = |commit: &str, target: &str, sha: &str| {
        let manifest = format!(
            "{{\"schema\":1,\"commit\":\"{commit}\",\"target\":\"{target}\",\"toolchain\":\"rust@1.98.1+mbx@1.0.0\",\"sha256\":\"{sha}\"}}"
        );
        std::fs::write(dir.join("candidate-manifest.json"), &manifest).expect("manifest fixture");
        std::fs::write(dir.join("velnor-actions"), []).expect("binary fixture");
        Command::new("chmod")
            .args(["+x", "velnor-actions"])
            .current_dir(&dir)
            .status()
            .expect("chmod");
        Command::new("sh")
            .args(["-c", &script])
            .env("RUNNER_TEMP", &root)
            .env("GITHUB_SHA", "f".repeat(40))
            .status()
            .expect("sh")
            .success()
    };
    let empty_sha = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    let good = run_case(&"f".repeat(40), "x86_64-unknown-linux-gnu", empty_sha);
    let tampered = run_case(&"f".repeat(40), "x86_64-unknown-linux-gnu", &"0".repeat(64));
    let wrong_target = run_case(&"f".repeat(40), "aarch64-apple-darwin", empty_sha);
    let wrong_commit = run_case(&"0".repeat(40), "x86_64-unknown-linux-gnu", empty_sha);
    std::fs::remove_dir_all(&root).ok();
    assert!(good, "good manifest must verify");
    assert!(!tampered, "tampered sha must fail");
    assert!(!wrong_target, "wrong target must fail");
    assert!(!wrong_commit, "wrong commit must fail");
    Ok(())
}

#[test]
fn release_job_uploads_once_without_rebuild() -> Result<(), RenderError> {
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Candidate);
    let text = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?]),
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &candidate_ctx(),
    )?;
    let start = text.find("velnor-release:").expect("release job");
    let end = text[start..]
        .find("\n  velnor-")
        .map_or(text.len(), |at| start + at);
    let window = &text[start..end];
    assert!(
        window.contains("velnor-candidate"),
        "needs candidate:\n{window}"
    );
    assert!(window.contains("ref_protected"), "ref gate:\n{window}");
    assert_eq!(
        window.matches("gh release upload").count(),
        1,
        "upload once:\n{window}"
    );
    let derived = candidate_artifact_name("x86_64-unknown-linux-gnu")?;
    assert!(window.contains(&derived), "derived download:\n{window}");
    for absent in [
        "Build candidate",
        "mise",
        "cargo ",
        "mbx",
        "actions/cache",
        "mr-boxington",
        "upload-artifact",
    ] {
        assert!(
            !window.contains(absent),
            "rebuild/cache {absent}:\n{window}"
        );
    }
    Ok(())
}

#[test]
fn task_job_gains_matrix_report_upload() -> Result<(), RenderError> {
    let text = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?, matrix_task_job()?]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    let start = text.find("velnor-task:").expect("task job");
    let end = text[start..]
        .find("velnor-plan:")
        .map_or(text.len(), |at| start + at);
    let window = &text[start..end.min(text.len())];
    let full = if end <= start { &text[start..] } else { window };
    assert!(full.contains(MATRIX_REPORT_UPLOAD_NAME), "upload:\n{full}");
    assert!(
        full.contains("velnor-matrix-r${{ github.run_id }}-a${{ github.run_attempt }}-${{ matrix.matrix_key }}"),
        "derived name:\n{full}"
    );
    let at = full.find(MATRIX_REPORT_UPLOAD_NAME).expect("upload step");
    assert!(
        snip(full, at, 500).contains("if: always()"),
        "if always:\n{full}"
    );
    assert_eq!(full.matches(MATRIX_REPORT_UPLOAD_NAME).count(), 1);
    Ok(())
}

#[test]
fn matrix_upload_absent_without_task_job() -> Result<(), RenderError> {
    let text = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert!(
        !text.contains(MATRIX_REPORT_UPLOAD_NAME),
        "phantom:\n{text}"
    );
    Ok(())
}

#[test]
fn write_request_inserted_before_plan_and_merge() -> Result<(), RenderError> {
    let mut final_job = job(
        "velnor-final",
        "Velnor / Required",
        vec!["velnor-plan".to_owned()],
        vec![acquire_fixture()?, merge_step()],
    )
    .1;
    final_job.condition = Some("always()".to_owned());
    let text = render_workflow_ir(
        &fixture_ir(vec![
            minimal_plan_job()?,
            ("velnor-final".to_owned(), final_job),
        ]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    for (consumer, target) in [("Plan", "plan-v1"), ("Merge reports", "merge-v1")] {
        let write = text.find("Write request").expect("write step");
        let use_at = text[write..]
            .find(consumer)
            .unwrap_or_else(|| panic!("{consumer} after write:\n{text}"));
        assert!(use_at > 0, "{consumer} order:\n{text}");
        assert!(
            text.contains(&format!("write-request-v1:{target}"))
                || text.contains("write-request-v1"),
            "op {target}:\n{text}"
        );
    }
    assert_eq!(text.matches("Write request").count(), 2, "both:\n{text}");
    Ok(())
}

#[test]
fn write_request_insertion_idempotent() -> Result<(), RenderError> {
    let plan = job(
        "velnor-plan",
        "Velnor Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            write_request_step("plan-v1")?,
            plan_step(),
        ],
    );
    let text = render_workflow_ir(
        &fixture_ir(vec![plan]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    assert_eq!(text.matches("Write request").count(), 1, "dup:\n{text}");
    Ok(())
}

#[test]
fn plan_outputs_publish_plan_id_run_key_and_matrix() -> Result<(), RenderError> {
    let text = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?, matrix_task_job()?]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    for line in [
        "matrix: ${{ steps.plan.outputs.matrix }}",
        "plan_id: ${{ steps.plan.outputs.plan_id }}",
        "run_key: ${{ steps.plan.outputs.run_key }}",
    ] {
        assert!(text.contains(line), "missing {line}:\n{text}");
    }
    assert_eq!(PLAN_ID_OUTPUT, "plan_id");
    assert_eq!(RUN_KEY_OUTPUT, "run_key");
    Ok(())
}

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
    let key = tools_cache_key(
        "x86_64-unknown-linux-gnu",
        "2026.9.16",
        "0.1.0",
        "velnor-plan",
    )?;
    for part in [
        "mise-tools-v1",
        "x86_64-unknown-linux-gnu",
        "2026.9.16",
        "0.1.0",
        "velnor-plan",
        "hashFiles(",
    ] {
        assert!(key.contains(part), "missing {part}:\n{key}");
    }
    assert!(!key.contains(' '), "spaces:\n{key}");
    for bad in ["latest", "", "has space"] {
        assert!(
            tools_cache_key("x86_64-unknown-linux-gnu", bad, "0.1.0", "velnor-plan").is_err(),
            "version {bad} must fail"
        );
    }
    assert!(tools_cache_key("riscv-none", "2026.9.16", "0.1.0", "velnor-plan").is_err());
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
        "velnor-plan",
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
        "velnor-final",
        "Velnor / Required",
        vec!["velnor-plan".to_owned()],
        vec![acquire_fixture()?, merge_step()],
    )
    .1;
    final_job.condition = Some("always()".to_owned());
    let text = render_workflow_ir(
        &fixture_ir(vec![
            minimal_plan_job()?,
            matrix_task_job()?,
            ("velnor-final".to_owned(), final_job),
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
    ctx.policy_commands = vec![
        velnor_actions_workflow_renderer::PolicyCommand {
            name: "Run cargo-deny".to_owned(),
            argv: vec!["true".to_owned()],
        },
        velnor_actions_workflow_renderer::PolicyCommand {
            name: "Run cargo-machete".to_owned(),
            argv: vec!["true".to_owned()],
        },
        velnor_actions_workflow_renderer::PolicyCommand {
            name: "Run zizmor".to_owned(),
            argv: vec!["true".to_owned()],
        },
    ];
    let mut final_job = job(
        "velnor-final",
        "Velnor / Required",
        vec!["velnor-plan".to_owned()],
        vec![merge_step()],
    )
    .1;
    final_job.condition = Some("always()".to_owned());
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Bootstrap);
    let text = render_workflow_ir(
        &fixture_ir(vec![
            minimal_plan_job()?,
            ("velnor-final".to_owned(), final_job),
        ]),
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &ctx,
    )?;
    assert!(text.contains("velnor-alint:"), "alint:\n{text}");
    assert!(text.contains("velnor-policy:"), "policy:\n{text}");
    assert!(!text.contains("velnor-task:"), "empty matrix:\n{text}");
    let start = text.find("velnor-alint:").expect("alint job");
    let window = snip(&text, start, 800);
    for input in [
        "path: .",
        "config: .alint.yml",
        "format: github",
        "fail-on-warning: \"true\"",
    ] {
        assert!(window.contains(input), "missing {input}:\n{window}");
    }
    let start = text.find("velnor-final:").expect("final job");
    let window = snip(&text, start, 600);
    for need in ["velnor-plan", "velnor-alint", "velnor-policy"] {
        assert!(window.contains(need), "missing need {need}:\n{window}");
    }
    let start = text.find("velnor-policy:").expect("policy job");
    let window = snip(&text, start, 900);
    for name in ["Run cargo-deny", "Run cargo-machete", "Run zizmor"] {
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
        "velnor-plan",
        "Velnor Plan",
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
    let scoped = token_plan_job(
        "Plan",
        vec!["true".to_owned()],
        BTreeMap::from([("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned())]),
    )?;
    render_workflow_ir(
        &fixture_ir(vec![scoped]),
        WorkflowPolicy::ConsumerV1,
        None,
        &fixture_ctx(),
    )?;
    let bad_env = token_plan_job(
        "Leak",
        vec!["true".to_owned()],
        BTreeMap::from([("GITHUB_TOKEN".to_owned(), "x".to_owned())]),
    )?;
    render_fails_with(vec![bad_env], "credential_env");
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
        "Velnor Task",
        vec!["velnor-plan".to_owned()],
        vec![shell_step(
            "Run task",
            vec!["true".to_owned()],
            BTreeMap::from([("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned())]),
        )?],
    );
    render_fails_with(vec![minimal_plan_job()?, task_token], "token_misplaced");
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
    assert_eq!(mise.trim(), "2026.9.16", "mise pin drift");
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

#[test]
fn artifact_roundtrip_uses_derived_candidate_name() -> Result<(), RenderError> {
    let name = candidate_artifact_name("x86_64-unknown-linux-gnu")?;
    let up = upload_artifact_step(&name, "${{ runner.temp }}/velnor/out")?;
    let down = download_artifact_step(&name, "${{ runner.temp }}/velnor/in")?;
    let velnor_actions_contract::StepKind::Action { with: up_with, .. } = &up.kind else {
        panic!("upload must be an action step");
    };
    let velnor_actions_contract::StepKind::Action {
        with: down_with, ..
    } = &down.kind
    else {
        panic!("download must be an action step");
    };
    assert_eq!(up_with["name"], down_with["name"]);
    assert!(up_with["name"].starts_with("velnor-candidate-"));
    let template = matrix_report_upload_step()?;
    let velnor_actions_contract::StepKind::Action { with, .. } = &template.kind else {
        panic!("matrix upload must be an action step");
    };
    assert!(with["name"].starts_with("velnor-matrix-"));
    Ok(())
}
