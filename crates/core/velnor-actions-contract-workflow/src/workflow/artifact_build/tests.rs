use super::{
    ArtifactBuildIdentity, ArtifactBuildMatrix, ArtifactBuildObservation, ArtifactBuildProvider,
    ArtifactBuildResult, ArtifactBuildRunContext, ArtifactBuildTaskPlan, DownloadedArtifactOutput,
    artifact_matrix_for_provider, artifact_name, expected_artifact_builds, export_artifact_result,
    reconcile_artifact_builds,
};
use crate::workflow::{
    JobConclusion, Plan, PlanBaseline, PlanGenerator, PlanMatrix, PlanRunner, Trust, WorkflowEvent,
};
use velnor_actions_contract_config::{
    ArtifactBuildOutput, ArtifactBuildTask, RunnerSelection, VerificationRunner,
};

fn task() -> ArtifactBuildTask {
    ArtifactBuildTask {
        id: "frontend-bundle".to_owned(),
        mise_task: "build-frontend".to_owned(),
        runner: VerificationRunner::LinuxX64,
        timeout_minutes: 30,
        outputs: vec![ArtifactBuildOutput {
            id: "bundle".to_owned(),
            path: "dist/app.tar".to_owned(),
            max_bytes: 1024,
        }],
    }
}

fn identity(provider: ArtifactBuildProvider, plan_digest: &str) -> ArtifactBuildIdentity {
    ArtifactBuildIdentity {
        repository_id: "1234567890".to_owned(),
        repository: "tailrocks/velnor-new".to_owned(),
        source_sha: "a4dfd62241cf85172a3c34ca5ab5e1750907d053".to_owned(),
        plan_digest: plan_digest.to_owned(),
        run_id: "123456789".to_owned(),
        run_attempt: 2,
        workflow_job_id: provider.workflow_job_id(true).to_owned(),
        provider,
        task_id: "frontend-bundle".to_owned(),
    }
}

fn run_context() -> ArtifactBuildRunContext {
    ArtifactBuildRunContext {
        repository_id: "1234567890".to_owned(),
        repository: "tailrocks/velnor-new".to_owned(),
        run_id: "123456789".to_owned(),
        run_attempt: 2,
    }
}

fn plan() -> Plan {
    Plan {
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
        baseline: PlanBaseline::unavailable(Some("no_baseline")).expect("baseline"),
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
            task: task(),
            providers: vec![
                ArtifactBuildProvider::GithubHosted,
                ArtifactBuildProvider::VelnorScaleSet,
            ],
        }],
        warnings: vec![],
        edges: vec![],
    }
}

fn observation(
    task: &ArtifactBuildTask,
    expected_identity: ArtifactBuildIdentity,
) -> ArtifactBuildObservation {
    let bytes = b"build bytes".to_vec();
    let result = export_artifact_result(
        expected_identity.clone(),
        task,
        &[("bundle".to_owned(), bytes.clone())],
    )
    .expect("declared nonempty file exports");
    let api_job_id = match expected_identity.provider {
        ArtifactBuildProvider::GithubHosted => 101,
        ArtifactBuildProvider::VelnorScaleSet => 102,
    };
    let api_artifact_id = match expected_identity.provider {
        ArtifactBuildProvider::GithubHosted => 201,
        ArtifactBuildProvider::VelnorScaleSet => 202,
    };
    let api_job_name = expected_identity
        .provider
        .workflow_job_name(&expected_identity.task_id);
    let api_artifact_name = artifact_name(&expected_identity).expect("artifact name");
    let (runner_id, runner_name, group_id, group_name, labels) = match expected_identity.provider {
        ArtifactBuildProvider::GithubHosted => {
            (None, None, None, None, vec!["ubuntu-26.04".to_owned()])
        }
        ArtifactBuildProvider::VelnorScaleSet => (
            Some(301),
            Some("velnor-runner-301".to_owned()),
            Some(12),
            Some("ChainArgos Scale Set".to_owned()),
            vec![
                "self-hosted".to_owned(),
                "velnor".to_owned(),
                "ubuntu-26.04-scale-set".to_owned(),
            ],
        ),
    };
    ArtifactBuildObservation {
        identity: expected_identity.clone(),
        api_run_id: expected_identity.run_id.clone(),
        api_head_sha: expected_identity.source_sha.clone(),
        api_job_id: Some(api_job_id),
        api_job_name,
        api_job_status: "completed".to_owned(),
        api_runner_id: runner_id,
        api_runner_name: runner_name,
        api_runner_group_id: group_id,
        api_runner_group_name: group_name,
        api_runner_labels: labels,
        api_artifact_id: Some(api_artifact_id),
        api_artifact_name: Some(api_artifact_name),
        api_artifact_size_bytes: Some(bytes.len() as u64),
        api_artifact_expired: Some(false),
        api_artifact_run_id: Some(expected_identity.run_id.clone()),
        api_artifact_repository_id: Some(expected_identity.repository_id.clone()),
        api_artifact_head_sha: Some(expected_identity.source_sha.clone()),
        conclusion: JobConclusion::Success,
        result: Some(result),
        downloaded_outputs: vec![DownloadedArtifactOutput {
            output_id: "bundle".to_owned(),
            path: "dist/app.tar".to_owned(),
            size_bytes: bytes.len() as u64,
            digest: velnor_actions_contract::canonical::digest_b3(&bytes),
        }],
    }
}

mod export;
mod plan;
mod reconcile;
