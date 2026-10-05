use crate::workflow::{
    QualificationCacheArtifact, QualificationCacheProducerContext, QualificationCacheRunMetadata,
    QualificationDispatch, QualificationRunRef,
};

pub(super) fn producer_context(
    plan: &crate::workflow::Plan,
) -> super::TestResult<QualificationCacheProducerContext> {
    let context = dispatch(plan)?;
    Ok(QualificationCacheProducerContext {
        repository: context.repository.clone(),
        default_branch: context.default_branch.clone(),
        git_ref: context.git_ref.clone(),
        ref_protected: context.ref_protected,
        workflow_ref: context.workflow_ref.clone(),
        workflow_sha: context.workflow_sha.clone(),
        source_sha: context.source_sha.clone(),
        run: run_ref(context),
    })
}

pub(super) fn run_metadata(
    plan: &crate::workflow::Plan,
) -> super::TestResult<QualificationCacheRunMetadata> {
    let context = dispatch(plan)?;
    Ok(QualificationCacheRunMetadata {
        repository: context.repository.clone(),
        default_branch: context.default_branch.clone(),
        git_ref: context.git_ref.clone(),
        ref_protected: context.ref_protected,
        workflow_path_ref: format!(
            "{}@{}",
            crate::workflow::CI_WORKFLOW_PATH,
            context.default_branch
        ),
        workflow_ref: context.workflow_ref.clone(),
        workflow_sha: context.workflow_sha.clone(),
        head_sha: context.source_sha.clone(),
        event: "workflow_dispatch".to_owned(),
        conclusion: "success".to_owned(),
        run: run_ref(context),
    })
}

pub(super) fn artifact_metadata(
    plan: &crate::workflow::Plan,
    size_bytes: usize,
) -> super::TestResult<QualificationCacheArtifact> {
    let context = dispatch(plan)?;
    Ok(QualificationCacheArtifact {
        id: context.run_id + 1_000,
        name: crate::workflow::QUALIFICATION_CACHE_RECEIPT_ARTIFACT.to_owned(),
        digest: format!("sha256:{:064x}", context.run_id),
        size_bytes: size_bytes as u64,
        expired: false,
        workflow_run_id: context.run_id,
        workflow_head_branch: context.default_branch.clone(),
        workflow_head_sha: context.source_sha.clone(),
    })
}

pub(super) fn dispatch(plan: &crate::workflow::Plan) -> super::TestResult<&QualificationDispatch> {
    plan.qualification
        .as_ref()
        .ok_or_else(|| std::io::Error::other("qualification context missing").into())
}

pub(super) fn run_ref(context: &QualificationDispatch) -> QualificationRunRef {
    QualificationRunRef {
        run_id: context.run_id,
        run_attempt: context.run_attempt,
    }
}
