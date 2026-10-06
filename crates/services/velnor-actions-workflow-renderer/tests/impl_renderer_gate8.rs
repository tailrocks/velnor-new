//! Gate-8 renderer cases: artifacts, manifest script, rehead, release.
use velnor_actions_contract_config::{GeneratorValidation, WorkflowPolicy};
use velnor_actions_contract_workflow::StepKind;
use velnor_actions_workflow_renderer::steps::{
    ARTIFACT_RETENTION_DAYS, candidate_manifest_script, download_artifact_step,
    rehead_actionlint_marker, upload_artifact_step,
};
use velnor_actions_workflow_renderer::{
    CANDIDATE_OUTPUT_DIR_EXPR, CANDIDATE_STAGE_DIR_EXPR, PRESEED_OUTPUT_DIR_EXPR,
    PRESEED_STAGE_DIR_EXPR, RenderError, candidate_artifact_name, checkout_step,
    crate_job_report_upload_step, matrix_report_upload_step, merge_step, plan_step,
    preseed_download_step, preseed_upload_step, publish_plan_step, render_workflow_ir,
    write_request_step,
};

use super::impl_renderer_fixtures::*;

#[test]
fn artifact_steps_pin_actions_and_reject_empty() -> Result<(), RenderError> {
    let name = candidate_artifact_name("x86_64-unknown-linux-gnu")?;
    let up = upload_artifact_step(&name, "${{ runner.temp }}/velnor/out").map(|step| step.name);
    assert_eq!(up, Ok("Upload candidate".to_owned()));
    let down = download_artifact_step(&name, "${{ runner.temp }}/velnor/in").map(|step| step.name);
    assert_eq!(down, Ok("Download candidate".to_owned()));
    assert!(upload_artifact_step("", "p").is_err());
    assert!(download_artifact_step("n", "").is_err());
    Ok(())
}

#[test]
fn every_upload_constructor_carries_typed_retention_days() -> Result<(), RenderError> {
    let name = candidate_artifact_name("x86_64-unknown-linux-gnu")?;
    let steps = [
        upload_artifact_step(&name, "${{ runner.temp }}/velnor/out")?,
        matrix_report_upload_step()?,
        crate_job_report_upload_step("rust-demo")?,
        publish_plan_step()?,
        preseed_upload_step()?,
    ];
    assert_eq!(ARTIFACT_RETENTION_DAYS, 30);
    for step in &steps {
        let StepKind::Action { uses, with, .. } = &step.kind else {
            panic!("upload must be an action step: {}", step.name);
        };
        assert!(uses.starts_with("actions/upload-artifact@"), "{uses}");
        assert_eq!(
            with.get("retention-days").map(String::as_str),
            Some(ARTIFACT_RETENTION_DAYS.to_string()).as_deref(),
            "{}",
            step.name
        );
    }
    Ok(())
}

#[test]
fn artifact_paths_reject_shell_expansions() {
    for shell in [
        "$RUNNER_TEMP/velnor/out",
        "${RUNNER_TEMP}/velnor/out",
        "x/$VAR/y",
        "trailing$",
    ] {
        assert!(
            upload_artifact_step("n", shell).is_err(),
            "upload accepted {shell}"
        );
        assert!(
            download_artifact_step("n", shell).is_err(),
            "download accepted {shell}"
        );
    }
    for ok in [
        "${{ runner.temp }}/velnor/out",
        "${{ matrix.dir }}",
        "relative/dir",
    ] {
        assert!(
            upload_artifact_step("n", ok).is_ok(),
            "upload rejected {ok}"
        );
        assert!(
            download_artifact_step("n", ok).is_ok(),
            "download rejected {ok}"
        );
    }
}

#[test]
fn rendered_action_inputs_carry_no_shell_expansions() -> Result<(), RenderError> {
    let mut ctx = fixture_ctx();
    ctx.validator_commands = validator_commands();
    ctx.candidate = Some(velnor_actions_workflow_renderer::CandidateSpec {
        build: mise_argv("rust@1.98.1", "mbx", &["build"]),
        qualify: vec!["sh".to_owned(), "-c".to_owned(), "true".to_owned()],
    });
    let plan = job(
        "plan",
        "Plan",
        Vec::new(),
        vec![
            checkout_step(&checkout_pin())?,
            acquire_fixture()?,
            write_request_step("plan-v1")?,
            plan_step(),
            preseed_upload_step()?,
        ],
    );
    let mut final_job = job(
        "required",
        "Required",
        vec!["plan".to_owned()],
        vec![
            preseed_download_step()?,
            acquire_fixture()?,
            write_request_step("merge-v1")?,
            merge_step(),
        ],
    );
    final_job.1.condition = Some("always()".to_owned());
    let support =
        WorkflowPolicy::VelnorRepositoryV1.support_workflow(GeneratorValidation::Candidate);
    let text = render_workflow_ir(
        &fixture_ir(vec![plan, final_job]),
        WorkflowPolicy::VelnorRepositoryV1,
        Some(&support),
        &ctx,
    )?;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("path:") || trimmed.starts_with("VELNOR_") {
            assert!(
                !line.contains("$RUNNER_TEMP"),
                "shell expansion in input: {line}"
            );
        }
    }
    for expr in [
        CANDIDATE_OUTPUT_DIR_EXPR,
        CANDIDATE_STAGE_DIR_EXPR,
        PRESEED_OUTPUT_DIR_EXPR,
        PRESEED_STAGE_DIR_EXPR,
    ] {
        assert!(text.contains(expr), "missing expression path: {expr}");
    }
    assert!(text.contains("VELNOR_STAGE_DIR: ${{ runner.temp }}/velnor/candidate"));
    Ok(())
}

#[test]
fn manifest_script_carries_contract_keys_without_substitution() {
    let script = candidate_manifest_script("x86_64-unknown-linux-gnu", "rust@1.98.1");
    for key in ["commit", "target", "toolchain", "sha256", "GITHUB_SHA"] {
        assert!(script.contains(key), "missing {key}");
    }
    assert!(!script.contains("$(") && !script.contains('`'));
}

#[test]
fn rehead_swaps_first_line_only() {
    let out =
        rehead_actionlint_marker("# old header\nbody: 1\n", "0.1.0").map_err(|err| err.to_string());
    assert!(out.is_ok_and(|text| text.ends_with("body: 1\n") && !text.contains("old header")));
    assert!(rehead_actionlint_marker("no-newline", "0.1.0").is_err());
}

#[test]
fn rehead_emits_the_single_spec_marker() {
    let text =
        rehead_actionlint_marker("# old header\nbody: 1\n", "0.1.0").expect("rehead keeps body");
    let first = text.lines().next().expect("first line");
    assert_eq!(
        first,
        "# Generated by Velnor Actions 0.1.0; edit .velnor/config.toml and regenerate."
    );
}
