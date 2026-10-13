//! F2 closure: candidate artifacts, verify-before-run, release.
use velnor_actions_contract::{GeneratorValidation, WorkflowPolicy};
use velnor_actions_workflow_renderer::steps::{
    candidate_attestation_script, download_artifact_step, matrix_report_upload_step,
    upload_artifact_step,
};
use velnor_actions_workflow_renderer::{
    CHECK_GENERATED_NAME, RenderError, VERIFY_MANIFEST_NAME, candidate_artifact_name,
    candidate_manifest_verify_script, candidate_manifest_verify_step, render_workflow_ir,
};

use super::impl_renderer_fixtures::*;
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
    assert!(!text.contains("name: candidate\n"), "fixed name:\n{text}");
    let script = candidate_manifest_verify_script("x86_64-unknown-linux-gnu");
    for token in [
        "schema",
        "commit",
        "target",
        "toolchain",
        "sha256",
        "GITHUB_SHA",
        "sha256sum",
        "line=; rest=;",
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
    let path = crate::required_tool_path::with_required_tool_path(&[])
        .map_err(|error| RenderError::InvalidWorkflow(format!("test_path:{error}")))?;
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
            .env("PATH", &path)
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
#[cfg(unix)]
fn candidate_attestation_script_attests_live_plan() -> Result<(), RenderError> {
    use std::process::Command;
    let script = candidate_attestation_script();
    for banned in ["$(", "`", "\n"] {
        assert!(!script.contains(banned), "banned {banned:?}:\n{script}");
    }
    let root = std::env::temp_dir().join(format!("velnor-attest-{}", std::process::id()));
    let run = root.join("velnor/r7-a2");
    let out = root.join("velnor/candidate-output");
    std::fs::create_dir_all(&run)
        .map_err(|err| RenderError::InvalidWorkflow(format!("tmp:{err}")))?;
    std::fs::create_dir_all(&out)
        .map_err(|err| RenderError::InvalidWorkflow(format!("tmp:{err}")))?;
    let run_case = |plan: Option<&str>| {
        let plan_path = run.join("plan.json");
        match plan {
            Some(body) => std::fs::write(&plan_path, body).expect("plan fixture"),
            None => {
                std::fs::remove_file(&plan_path).ok();
            }
        }
        std::fs::remove_file(out.join("candidate-attestation.json")).ok();
        let ok = Command::new("sh")
            .args(["-c", &script])
            .env("RUNNER_TEMP", &root)
            .env("GITHUB_RUN_ID", "7")
            .env("GITHUB_RUN_ATTEMPT", "2")
            .status()
            .expect("sh")
            .success();
        let attested = std::fs::read_to_string(out.join("candidate-attestation.json")).ok();
        (ok, attested)
    };
    let head = "a".repeat(40);
    let plan = format!(
        "{{\"schema\":1,\"run_key\":\"r7-a2\",\"plan_id\":\"plan-r7-a2\",\"base\":null,\"head\":\"{head}\",\"event\":\"push\"}}"
    );
    let (ok, attested) = run_case(Some(&plan));
    assert!(ok, "good plan must attest");
    assert_eq!(
        attested.as_deref(),
        Some(format!("{{\"schema\":1,\"commit\":\"{head}\"}}").as_str()),
        "attestation binds the head"
    );
    let (ok, _) = run_case(None);
    assert!(!ok, "missing plan must fail");
    let (ok, _) = run_case(Some("{\"schema\":1,\"head\":\"\"}"));
    assert!(!ok, "empty head must fail");
    // A quoted head truncates at extraction: whatever the script emits
    // must not equal the binding for the real head (merge closes it).
    let (ok, attested) = run_case(Some("{\"schema\":1,\"head\":\"ab\\\"cd\"}"));
    assert!(
        !ok || attested.as_deref() != Some("{\"schema\":1,\"commit\":\"ab\\\"cd\"}"),
        "quoted head must not bind: {attested:?}"
    );
    std::fs::remove_dir_all(&root).ok();
    Ok(())
}

#[test]
fn candidate_job_downloads_plan_and_attests_before_upload() -> Result<(), RenderError> {
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Candidate);
    let text = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?]),
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &candidate_ctx(),
    )?;
    let order = [
        "Download plan",
        "Write candidate manifest",
        "Write candidate attestation",
        "Upload candidate",
        "Download candidate",
        VERIFY_MANIFEST_NAME,
        "Qualify candidate",
    ];
    let candidate_at = text.find("candidate:").expect("candidate job");
    let mut at = candidate_at;
    for name in order {
        let found = text[at..]
            .find(name)
            .unwrap_or_else(|| panic!("missing {name}:\n{text}"));
        at += found + name.len();
    }
    let plain_support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Bootstrap);
    let mut plain_ctx = fixture_ctx();
    plain_ctx.validator_commands = validator_commands();
    let plain = render_workflow_ir(
        &fixture_ir(vec![minimal_plan_job()?]),
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&plain_support),
        &plain_ctx,
    )?;
    assert!(
        !plain.contains("Write candidate attestation"),
        "non-candidate mode emits no attestation:\n{plain}"
    );
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
    let start = text.find("release:").expect("release job");
    let end = text[start..]
        .find("\n  velnor-")
        .map_or(text.len(), |at| start + at);
    let window = &text[start..end];
    assert!(window.contains("candidate"), "needs candidate:\n{window}");
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
