// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, fs, path::Path, process::Command};
use velnor_actions_contract::{Job, StepKind};

pub(super) fn assert_prefetch(job: &Job) -> Result<(), Box<dyn Error>> {
    let restore = job
        .steps
        .iter()
        .position(|step| step.id == Some(velnor_actions_contract::StepId::SourcesRestore))
        .ok_or("source restore ID missing")?;
    let prefetch = job
        .steps
        .iter()
        .position(|step| step.name == "Sanitize and prefetch Cargo sources")
        .ok_or("source prefetch missing")?;
    let run = job
        .steps
        .iter()
        .position(|step| step.name == "Run runner image candidate")
        .ok_or("family task missing")?;
    assert!(
        restore < prefetch && prefetch < run,
        "restored sources must be sanitized before use"
    );
    let StepKind::Shell { run, env } = &job.steps[prefetch].kind else {
        return Err("prefetch is not a shell command".into());
    };
    assert!(run.iter().any(|part| {
        part.contains(
            "source-qualification.sh prefetch-locked-sources-for-qualification-workspaces",
        )
    }));
    assert!(
        run.iter()
            .any(|part| part.contains("export MISE_DATA_DIR=\"$task_root/data\"")),
        "prefetch does not use the installed private tool home"
    );
    assert!(
        run.iter().any(|part| part.contains(
            "velnor-verification-${GITHUB_RUN_ID}-${GITHUB_RUN_ATTEMPT}-runner-image-family"
        )),
        "prefetch uses a different task home"
    );
    assert_eq!(
        env.get("VELNOR_PREFETCH_RUST_ONLY").map(String::as_str),
        Some("1")
    );
    assert_eq!(
        env.get("SOURCES_CACHE_HIT").map(String::as_str),
        Some("${{ steps.sources-restore.outputs.cache-hit }}")
    );
    assert_eq!(
        env.get("SOURCES_CACHE_MATCHED_KEY").map(String::as_str),
        Some("${{ steps.sources-restore.outputs.cache-matched-key }}")
    );
    assert!(job.steps[prefetch].condition.is_none());
    Ok(())
}

pub(super) fn assert_evidence_symlink_rejected(
    job: &Job,
    root: &Path,
) -> Result<(), Box<dyn Error>> {
    use std::os::unix::fs::symlink;
    let step = job
        .steps
        .iter()
        .find(|step| step.name == "Sanitize and prefetch Cargo sources")
        .ok_or("source prefetch missing")?;
    let StepKind::Shell { run, env } = &step.kind else {
        return Err("prefetch is not a shell command".into());
    };
    let (program, args) = run.split_first().ok_or("prefetch command empty")?;
    let owned = tempfile::tempdir()?;
    let runner_temp = owned.path().canonicalize()?;
    let foreign = tempfile::tempdir()?;
    let canary = foreign.path().join("canary");
    fs::write(&canary, b"preserved")?;
    symlink(
        foreign.path(),
        runner_temp.join("source-qualification-artifacts"),
    )?;
    let status = Command::new(program)
        .args(args)
        .envs(env)
        .env("RUNNER_TEMP", &runner_temp)
        .env("GITHUB_WORKSPACE", root)
        .env("GITHUB_RUN_ID", "19")
        .env("GITHUB_RUN_ATTEMPT", "1")
        .env("GITHUB_SHA", "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        .current_dir(root)
        .output()?
        .status;
    assert!(
        !status.success(),
        "prefetch accepted an occupied evidence path"
    );
    assert_eq!(fs::read(canary)?, b"preserved", "foreign canary changed");
    assert!(
        !foreign.path().join("logs").exists(),
        "prefetch wrote into foreign evidence directory"
    );
    Ok(())
}

pub(super) fn assert_actual_guard(
    job: &Job,
    root: &Path,
    original: &[u8],
) -> Result<(), Box<dyn Error>> {
    let step = job
        .steps
        .iter()
        .find(|step| step.name == "Verify runner family task source")
        .ok_or("source guard missing")?;
    let StepKind::Shell { run, env } = &step.kind else {
        return Err("source guard is not a shell command".into());
    };
    let (program, args) = run.split_first().ok_or("source guard command empty")?;
    let tools = tempfile::tempdir()?;
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::fs::PermissionsExt;
        let adapter = tools.path().join("sha256sum");
        fs::write(&adapter, "#!/bin/sh\nexec /usr/bin/shasum -a 256 \"$@\"\n")?;
        fs::set_permissions(adapter, fs::Permissions::from_mode(0o700))?;
    }
    let path = format!("{}:{}", tools.path().display(), std::env::var("PATH")?);
    let marker = tools.path().join("task-reached");
    let invoke = || -> Result<bool, Box<dyn Error>> {
        let output = Command::new(program)
            .args(args)
            .envs(env)
            .env("PATH", &path)
            .current_dir(root)
            .output()?;
        if output.status.success() {
            fs::write(&marker, b"task reached")?;
        }
        Ok(output.status.success())
    };
    assert!(invoke()?, "valid source rejected by rendered guard");
    fs::remove_file(&marker)?;
    for name in [
        "scripts/check-runner-image-candidate.sh",
        "scripts/generator-release/cargo-git-handoff.py",
        "scripts/generator-release/cargo-source-handoff.sh",
        "scripts/generator-release/source-qualification.sh",
    ] {
        let script = root.join(name);
        let saved = if name == "scripts/check-runner-image-candidate.sh" {
            original.to_vec()
        } else {
            fs::read(&script)?
        };
        fs::write(&script, b"modified source")?;
        let accepted = invoke()?;
        fs::write(&script, saved)?;
        assert!(
            !accepted,
            "modified source passed the rendered guard: {name}"
        );
        assert!(
            !marker.exists(),
            "modified source reached the family task: {name}"
        );
    }
    Ok(())
}

pub(super) fn assert_binding_rejections(
    prepared: &velnor_actions_orchestrator::GenerationPreparation,
    job: &Job,
) -> Result<(), Box<dyn Error>> {
    let mut original = job
        .steps
        .iter()
        .find(|step| step.name == "Sanitize and prefetch Cargo sources")
        .ok_or("prefetch missing")?
        .clone();
    let StepKind::Shell { env, .. } = &mut original.kind else {
        return Err("expected shell".into());
    };
    let rust = prepared
        .catalog
        .version(velnor_actions_mise::PinnedTool::Rust)
        .to_owned();
    for name in ["RUSTUP_TOOLCHAIN", "VELNOR_RUST_VERSION"] {
        assert_eq!(
            env.get(name).map(String::as_str),
            Some("${VELNOR_TOOL_RUST}")
        );
        env.insert(name.into(), rust.clone());
    }
    for (hit, matched, role, change_command, expected) in [
        (
            "${{ steps.sources-restore.outputs.cache-hit }}",
            "${{ steps.sources-restore.outputs.cache-matched-key }}",
            original.role,
            false,
            None,
        ),
        (
            "${{ steps.sources-restore.outputs.cache-matched-key }}",
            "${{ steps.sources-restore.outputs.cache-hit }}",
            original.role,
            false,
            Some("runner_family_prefetch_binding"),
        ),
        (
            "${{ github.sha }}",
            "${{ steps.sources-restore.outputs.cache-matched-key }}",
            original.role,
            false,
            Some("runner_family_prefetch_binding"),
        ),
        (
            "${{ steps.sources-restore.outputs.cache-hit }}",
            "${{ steps.sources-restore.outputs.cache-matched-key }}",
            None,
            false,
            Some("bad_env_expression:SOURCES_CACHE_HIT"),
        ),
        (
            "${{ steps.sources-restore.outputs.cache-hit }}",
            "${{ steps.sources-restore.outputs.cache-matched-key }}",
            original.role,
            true,
            Some("runner_family_prefetch_binding"),
        ),
    ] {
        let mut candidate = original.clone();
        candidate.role = role;
        let StepKind::Shell { run, env } = &mut candidate.kind else {
            return Err("expected shell".into());
        };
        env.insert("SOURCES_CACHE_HIT".into(), hit.into());
        env.insert("SOURCES_CACHE_MATCHED_KEY".into(), matched.into());
        if change_command {
            run[2].push_str("; true");
        }
        let result = render_prefetch_fixture(prepared, job, candidate);
        if let Some(expected) = expected {
            let error = result.err().ok_or("unbound cache output accepted")?;
            assert!(
                error.to_string().contains(expected),
                "wrong rejection: {error}"
            );
        } else {
            result?;
        }
    }
    Ok(())
}

fn render_prefetch_fixture(
    prepared: &velnor_actions_orchestrator::GenerationPreparation,
    job: &Job,
    candidate: velnor_actions_contract::Step,
) -> Result<String, Box<dyn Error>> {
    let mut ir = prepared.workflow.ir.clone();
    let mut fixture_job = job.clone();
    fixture_job.steps = vec![candidate];
    fixture_job.needs.clear();
    fixture_job.condition = None;
    fixture_job.env = velnor_actions_contract::JobEnv::Unset;
    ir.jobs.clear();
    ir.jobs.insert("prefetch-fixture".into(), fixture_job);
    let mut context = prepared.workflow.context.clone();
    context.workflow_tasks.clear();
    context.lane_plan = None;
    context.validator_commands.clear();
    context.preseed = false;
    context.tool_pins.clear();
    context.standalone_tools_donors.clear();
    context.non_inputs = velnor_actions_contract::NonInputs::default();
    let runtime = velnor_actions_workflow_renderer::render::CanonicalRuntimeInputs::new(
        context.plan_consumer_env.clone(),
        context.controller.clone(),
        context.request_dir.clone(),
        context.checkout_uses.clone(),
        false,
    )?;
    context.authority_steps =
        velnor_actions_workflow_renderer::render::CanonicalAuthoritySteps::qualify_jobs(
            &ir.jobs,
            velnor_actions_contract::WorkflowPolicy::ConsumerV1,
            &prepared.config,
            &context.runs_on,
            Vec::new(),
            runtime,
        )?;
    Ok(
        velnor_actions_workflow_renderer::render::render_workflow_ir(
            &ir,
            velnor_actions_contract::WorkflowPolicy::ConsumerV1,
            None,
            &context,
        )?,
    )
}
