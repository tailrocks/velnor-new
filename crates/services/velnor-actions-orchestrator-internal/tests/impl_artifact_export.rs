//! Filesystem and identity checks for streamed artifact output staging.

use std::fs;
use std::path::Path;
use std::{error::Error, io};

use velnor_actions_contract::canonical::{canonical_json_str, digest_b3, digest_b3_typed};
use velnor_actions_contract_config::{
    ArtifactBuildOutput, ArtifactBuildTask, RunnerSelection, VerificationRunner,
};
use velnor_actions_contract_workflow::{
    ArtifactBuildIdentity, ArtifactBuildProvider, ArtifactBuildResult, ArtifactBuildRunContext,
    ArtifactBuildTaskPlan, Plan, PlanBaseline, PlanGenerator, PlanMatrix, PlanRunner, Trust,
    WorkflowEvent, canonical_plan_digest,
};
use velnor_actions_orchestrator_core::OrchestratorError;
use velnor_actions_orchestrator_internal::artifact_export::{
    ARTIFACT_OUTPUTS_DIRECTORY, ARTIFACT_RESULT_FILENAME, ArtifactExportInvocation,
    materialize_artifact, materialize_planned_artifact,
};

type TestResult = Result<(), Box<dyn Error>>;

fn task(max_bytes: u64) -> ArtifactBuildTask {
    ArtifactBuildTask {
        id: "linux-image".to_owned(),
        mise_task: "ci-build-linux-image".to_owned(),
        runner: VerificationRunner::LinuxX64,
        timeout_minutes: 60,
        outputs: vec![ArtifactBuildOutput {
            id: "image-tar".to_owned(),
            path: "dist/image.tar".to_owned(),
            max_bytes,
        }],
    }
}

fn identity() -> ArtifactBuildIdentity {
    ArtifactBuildIdentity {
        repository_id: "1234567890".to_owned(),
        repository: "tailrocks/velnor-new".to_owned(),
        source_sha: "a".repeat(40),
        plan_digest: digest_b3_typed(b"canonical plan").as_str().to_owned(),
        run_id: "123456789".to_owned(),
        run_attempt: 2,
        workflow_job_id: "artifact-build".to_owned(),
        provider: velnor_actions_contract_workflow::ArtifactBuildProvider::GithubHosted,
        task_id: "linux-image".to_owned(),
    }
}

fn plan() -> Result<Plan, Box<dyn Error>> {
    Ok(Plan {
        schema: Plan::SCHEMA,
        run_key: "r123456789-a2".to_owned(),
        plan_id: "plan-r123456789-a2".to_owned(),
        base: None,
        head: "a4dfd62241cf85172a3c34ca5ab5e1750907d053".to_owned(),
        event: WorkflowEvent::Push,
        runner: PlanRunner {
            label: "ubuntu-26.04".to_owned(),
            selection: RunnerSelection::LatestDefault,
        },
        trust: Trust::Trusted,
        baseline: PlanBaseline::unavailable(Some("no_baseline"))?,
        generator: PlanGenerator {
            version: "0.1.1".to_owned(),
            target: "x86_64-unknown-linux-gnu".to_owned(),
            sha256: "ab".repeat(32),
        },
        packages: vec![],
        obligations: vec![],
        matrix: PlanMatrix { include: vec![] },
        task_ids: vec![],
        artifact_tasks: vec![ArtifactBuildTaskPlan {
            task: task(1024),
            providers: vec![
                ArtifactBuildProvider::GithubHosted,
                ArtifactBuildProvider::VelnorScaleSet,
            ],
        }],
        warnings: vec![],
        edges: vec![],
    })
}

fn invocation(
    plan: &Plan,
    provider: ArtifactBuildProvider,
) -> Result<ArtifactExportInvocation, Box<dyn Error>> {
    let context = ArtifactBuildRunContext {
        repository_id: "1234567890".to_owned(),
        repository: "tailrocks/velnor-new".to_owned(),
        run_id: "123456789".to_owned(),
        run_attempt: 2,
    };
    let identity = velnor_actions_contract_workflow::expected_artifact_builds(
        plan,
        &context,
        &plan.artifact_tasks[0].providers,
    )?
    .into_iter()
    .find(|expected| expected.identity.provider == provider)
    .ok_or_else(|| io::Error::other("provider identity missing"))?
    .identity;
    let artifact_name = velnor_actions_contract_workflow::artifact_name(&identity)?;
    Ok(ArtifactExportInvocation {
        repository_id: context.repository_id,
        repository: context.repository,
        github_sha: plan.head.clone(),
        source_sha: plan.head.clone(),
        plan_digest: canonical_plan_digest(plan)?,
        run_id: context.run_id,
        run_attempt: context.run_attempt,
        workflow_job_id: identity.workflow_job_id,
        runner_environment: match provider {
            ArtifactBuildProvider::GithubHosted => "github-hosted",
            ArtifactBuildProvider::VelnorScaleSet => "self-hosted",
        }
        .to_owned(),
        runner_os: "Linux".to_owned(),
        runner_arch: "X64".to_owned(),
        provider,
        task_id: "linux-image".to_owned(),
        mise_task: "ci-build-linux-image".to_owned(),
        artifact_name,
    })
}

fn setup(root: &Path, bytes: &[u8]) -> io::Result<()> {
    let output = root.join("dist/image.tar");
    let parent = output
        .parent()
        .ok_or_else(|| io::Error::other("fixture output has no parent"))?;
    fs::create_dir_all(parent)?;
    fs::write(output, bytes)
}

fn error_text<T: std::fmt::Debug>(result: Result<T, OrchestratorError>) -> String {
    match result {
        Err(error) => error.to_string(),
        Ok(value) => format!("unexpected success: {value:?}"),
    }
}

#[test]
fn materializes_exact_identity_manifest_and_streamed_output() -> TestResult {
    let source = tempfile::tempdir()?;
    let temp = tempfile::tempdir()?;
    let contents = b"small deterministic image fixture";
    setup(source.path(), contents)?;

    let artifact = materialize_artifact(source.path(), temp.path(), &task(1024), identity())?;
    let stored: ArtifactBuildResult = serde_json::from_slice(&fs::read(
        artifact.directory.join(ARTIFACT_RESULT_FILENAME),
    )?)?;
    assert_eq!(stored, artifact.result);
    assert_eq!(stored.identity.source_sha, "a".repeat(40));
    assert_eq!(stored.identity.provider.token(), "hosted");
    assert_eq!(stored.identity.task_id, "linux-image");
    assert_eq!(stored.outputs[0].size_bytes, contents.len() as u64);
    assert_eq!(stored.outputs[0].digest, digest_b3(contents));
    assert_eq!(
        fs::read(
            artifact
                .directory
                .join(ARTIFACT_OUTPUTS_DIRECTORY)
                .join("image-tar.bin")
        )?,
        contents
    );
    Ok(())
}

#[test]
fn oversize_output_does_not_publish_result_or_keep_partial_file() -> TestResult {
    let source = tempfile::tempdir()?;
    let temp = tempfile::tempdir()?;
    setup(source.path(), b"123456789")?;
    let artifact = error_text(materialize_artifact(
        source.path(),
        temp.path(),
        &task(8),
        identity(),
    ));
    assert!(artifact.contains("oversize"), "{artifact}");
    let expected_dir = temp
        .path()
        .join("velnor/artifact-builds/velnor-build-123456789-2-hosted-linux-image");
    assert!(!expected_dir.join(ARTIFACT_RESULT_FILENAME).exists());
    assert!(
        !expected_dir
            .join(ARTIFACT_OUTPUTS_DIRECTORY)
            .join("image-tar.bin")
            .exists()
    );
    Ok(())
}

#[test]
fn absent_and_empty_outputs_fail_without_a_result_manifest() -> TestResult {
    let source = tempfile::tempdir()?;
    let temp = tempfile::tempdir()?;
    let absent = error_text(materialize_artifact(
        source.path(),
        temp.path(),
        &task(1024),
        identity(),
    ));
    assert!(absent.contains("not_found"), "{absent}");

    setup(source.path(), b"")?;
    let empty = error_text(materialize_artifact(
        source.path(),
        temp.path(),
        &task(1024),
        identity(),
    ));
    assert!(empty.contains("artifact_empty_output"), "{empty}");
    let expected_dir = temp
        .path()
        .join("velnor/artifact-builds/velnor-build-123456789-2-hosted-linux-image");
    assert!(!expected_dir.join(ARTIFACT_RESULT_FILENAME).exists());
    Ok(())
}

#[cfg(unix)]
#[test]
fn symlinked_declared_output_is_refused() -> TestResult {
    use std::os::unix::fs::symlink;

    let source = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let temp = tempfile::tempdir()?;
    let target = outside.path().join("image.tar");
    fs::write(&target, b"must not read")?;
    let link = source.path().join("dist/image.tar");
    let parent = link
        .parent()
        .ok_or_else(|| io::Error::other("fixture link has no parent"))?;
    fs::create_dir_all(parent)?;
    symlink(target, link)?;

    let error = error_text(materialize_artifact(
        source.path(),
        temp.path(),
        &task(1024),
        identity(),
    ));
    assert!(error.contains("symlink_refused"), "{error}");
    Ok(())
}

#[cfg(unix)]
#[test]
fn intermediate_symlinked_declared_output_is_refused_without_a_result() -> TestResult {
    use std::os::unix::fs::symlink;

    let source = tempfile::tempdir()?;
    let outside = tempfile::tempdir()?;
    let temp = tempfile::tempdir()?;
    fs::write(outside.path().join("image.tar"), b"outside output")?;
    symlink(outside.path(), source.path().join("dist"))?;

    let error = error_text(materialize_artifact(
        source.path(),
        temp.path(),
        &task(1024),
        identity(),
    ));
    assert!(
        error.contains("symlink_refused") || error.contains("not_a_directory"),
        "{error}"
    );
    let expected_dir = temp
        .path()
        .join("velnor/artifact-builds/velnor-build-123456789-2-hosted-linux-image");
    assert!(!expected_dir.join(ARTIFACT_RESULT_FILENAME).exists());
    assert!(
        !expected_dir
            .join(ARTIFACT_OUTPUTS_DIRECTORY)
            .join("image-tar.bin")
            .exists()
    );
    Ok(())
}

#[test]
fn preexisting_artifact_directory_is_not_overwritten() -> TestResult {
    let source = tempfile::tempdir()?;
    let temp = tempfile::tempdir()?;
    setup(source.path(), b"fixture")?;
    let path = temp.path().join(
        "velnor/artifact-builds/velnor-build-123456789-2-hosted-linux-image/outputs/image-tar.bin",
    );
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("fixture output has no parent"))?;
    fs::create_dir_all(parent)?;
    fs::write(&path, b"preexisting")?;
    let error = error_text(materialize_artifact(
        source.path(),
        temp.path(),
        &task(1024),
        identity(),
    ));
    assert!(error.contains("artifact_output_exists"), "{error}");
    assert_eq!(fs::read(path)?, b"preexisting");
    Ok(())
}

#[test]
fn plan_bound_export_captures_the_exact_provider_task_and_output() -> TestResult {
    let source = tempfile::tempdir()?;
    let temp = tempfile::tempdir()?;
    let contents = b"official task output";
    setup(source.path(), contents)?;
    let plan = plan()?;
    let artifact = materialize_planned_artifact(
        source.path(),
        temp.path(),
        &canonical_json_str(&plan)?,
        invocation(&plan, ArtifactBuildProvider::VelnorScaleSet)?,
    )?;
    assert_eq!(
        artifact.result.identity.provider,
        ArtifactBuildProvider::VelnorScaleSet
    );
    assert_eq!(
        artifact.result.identity.workflow_job_id,
        "artifact-build__local"
    );
    assert_eq!(
        artifact.result.artifact_name,
        "velnor-build-123456789-2-velnor-linux-image"
    );
    assert_eq!(artifact.result.outputs[0].digest, digest_b3(contents));
    Ok(())
}

#[test]
fn plan_bound_export_rejects_mismatched_source_before_staging_outputs() -> TestResult {
    let source = tempfile::tempdir()?;
    let temp = tempfile::tempdir()?;
    setup(source.path(), b"must not stage")?;
    let plan = plan()?;
    let mut invocation = invocation(&plan, ArtifactBuildProvider::GithubHosted)?;
    invocation.source_sha = "b".repeat(40);
    let error = error_text(materialize_planned_artifact(
        source.path(),
        temp.path(),
        &canonical_json_str(&plan)?,
        invocation,
    ));
    assert!(error.contains("artifact_plan_identity_mismatch"), "{error}");
    assert!(!temp.path().join("velnor/artifact-builds").exists());
    Ok(())
}
