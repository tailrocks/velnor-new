use super::*;

fn mixed_plan() -> Result<Plan, Box<dyn Error>> {
    let mut plan = plan()?;
    for (id, providers) in [
        (
            "verify-both",
            vec![
                ArtifactBuildProvider::GithubHosted,
                ArtifactBuildProvider::VelnorScaleSet,
            ],
        ),
        ("verify-hosted", vec![ArtifactBuildProvider::GithubHosted]),
        ("verify-scale", vec![ArtifactBuildProvider::VelnorScaleSet]),
    ] {
        let mut static_task = task(1024);
        static_task.id = id.to_owned();
        static_task.mise_task = format!("check-{id}-output");
        plan.artifact_tasks.push(ArtifactBuildTaskPlan {
            task: static_task,
            producer: velnor_actions_contract_workflow::ArtifactBuildProducer::VerificationTask,
            providers,
        });
    }
    plan.validate()?;
    Ok(plan)
}

fn static_invocation(
    plan: &Plan,
    task_id: &str,
    provider: ArtifactBuildProvider,
) -> Result<ArtifactExportInvocation, Box<dyn Error>> {
    let context = ArtifactBuildRunContext {
        repository_id: "1234567890".to_owned(),
        repository: "tailrocks/velnor-new".to_owned(),
        run_id: "123456789".to_owned(),
        run_attempt: 2,
    };
    let providers = velnor_actions_contract_workflow::artifact_plan_providers(plan);
    let expected =
        velnor_actions_contract_workflow::expected_artifact_builds(plan, &context, &providers)?
            .into_iter()
            .find(|item| {
                item.producer
                    == velnor_actions_contract_workflow::ArtifactBuildProducer::VerificationTask
                    && item.identity.task_id == task_id
                    && item.identity.provider == provider
            })
            .ok_or_else(|| io::Error::other("static task/provider pair is not planned"))?;
    let identity = expected.identity;
    let artifact_name = velnor_actions_contract_workflow::artifact_name(&identity)?;
    let task = plan
        .artifact_tasks
        .iter()
        .find(|item| item.task.id == task_id)
        .ok_or_else(|| io::Error::other("static task is not planned"))?;
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
        task_id: task_id.to_owned(),
        mise_task: task.task.mise_task.clone(),
        artifact_name,
        producer: velnor_actions_contract_workflow::ArtifactBuildProducer::VerificationTask,
    })
}

#[test]
fn exporter_routes_mixed_static_scopes_and_rejects_unadmitted_lanes() -> TestResult {
    let source = tempfile::tempdir()?;
    let temp = tempfile::tempdir()?;
    let contents = b"hosted verification result";
    setup(source.path(), contents)?;
    let plan = mixed_plan()?;
    let plan_text = canonical_json_str(&plan)?;

    for (task_id, provider, expected_job) in [
        (
            "verify-hosted",
            ArtifactBuildProvider::GithubHosted,
            "task-verify-hosted",
        ),
        (
            "verify-scale",
            ArtifactBuildProvider::VelnorScaleSet,
            "task-verify-scale",
        ),
        (
            "verify-both",
            ArtifactBuildProvider::GithubHosted,
            "task-verify-both__hosted",
        ),
        (
            "verify-both",
            ArtifactBuildProvider::VelnorScaleSet,
            "task-verify-both__local",
        ),
    ] {
        let exported = materialize_planned_artifact(
            source.path(),
            temp.path(),
            &plan_text,
            static_invocation(&plan, task_id, provider)?,
        )?;
        assert_eq!(exported.result.identity.provider, provider);
        assert_eq!(exported.result.identity.task_id, task_id);
        assert_eq!(exported.result.identity.workflow_job_id, expected_job);
        assert_eq!(exported.result.outputs[0].digest, digest_b3(contents));
    }

    for (task_id, admitted, rejected) in [
        (
            "verify-hosted",
            ArtifactBuildProvider::GithubHosted,
            ArtifactBuildProvider::VelnorScaleSet,
        ),
        (
            "verify-scale",
            ArtifactBuildProvider::VelnorScaleSet,
            ArtifactBuildProvider::GithubHosted,
        ),
    ] {
        let mut unadmitted = static_invocation(&plan, task_id, admitted)?;
        unadmitted.provider = rejected;
        unadmitted.runner_environment = match rejected {
            ArtifactBuildProvider::GithubHosted => "github-hosted",
            ArtifactBuildProvider::VelnorScaleSet => "self-hosted",
        }
        .to_owned();
        let error = error_text(materialize_planned_artifact(
            source.path(),
            temp.path(),
            &plan_text,
            unadmitted,
        ));
        assert!(error.contains("artifact_task_not_in_plan"), "{error}");
    }
    Ok(())
}
