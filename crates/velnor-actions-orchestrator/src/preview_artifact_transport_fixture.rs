//! Pure native-transport fixtures; no source task, process or network access.
use super::*;
use velnor_actions_contract::{
    PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlan, PackageReleasePlanProduct,
    package_release::{PackageReleaseChannel, PackageReleasePayloadFormat},
    preview_production::{
        PREVIEW_PRODUCTION_SCHEMA_V1, PreviewPayloadPair, PreviewProductionContext,
        preview_plan_sha256,
    },
};

pub(in crate::package_release_publish) fn request() -> PreviewProductionRequest {
    let plan = PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.into(),
        channel: PackageReleaseChannel::Preview,
        repository: "example/app".into(),
        default_branch: "main".into(),
        base_version: "0.6.4".into(),
        products: vec![PackageReleasePlanProduct {
            id: "app".into(),
            package: "app".into(),
            bins: vec!["app".into()],
            targets: vec!["aarch64-apple-darwin".into()],
            format: PackageReleasePayloadFormat::TarGz,
        }],
        companions: vec![],
    };
    PreviewProductionRequest {
        schema: PREVIEW_PRODUCTION_SCHEMA_V1.into(),
        plan_sha256: preview_plan_sha256(&plan).expect("plan"),
        plan,
        context: PreviewProductionContext {
            repository_id: 101,
            head_repository_id: 101,
            source_sha: "a".repeat(40),
            source_ref: "refs/heads/main".into(),
            workflow_authority_sha: "a".repeat(40),
            run_id: 202,
            run_attempt: 2,
            ci_run_id: 201,
            ci_run_attempt: 1,
            commit_count: 1306,
            version: "0.6.4-preview.1306+aaaaaaa".into(),
        },
        stage: PreviewProductionStage::Assemble {
            inputs: vec![PayloadArtifactRef {
                pair: PreviewPayloadPair {
                    product_id: "app".into(),
                    target: "aarch64-apple-darwin".into(),
                },
                artifact_id: 303,
                sha256: sha256(b"closed ZIP bytes"),
            }],
        },
    }
}

pub(in crate::package_release_publish) fn record() -> ArtifactRecord {
    let request = request();
    let PreviewProductionStage::Assemble { inputs } = &request.stage else {
        unreachable!()
    };
    ArtifactRecord {
        id: 303,
        name: preview_payload_artifact_name(&request.plan, &inputs[0].pair, &request.context)
            .expect("name"),
        expired: false,
        size_in_bytes: 16,
        digest: Some(format!("sha256:{}", inputs[0].sha256)),
        workflow_run: Some(ArtifactSource {
            id: 202,
            repository_id: 101,
            head_repository_id: 101,
            head_sha: "a".repeat(40),
            head_branch: "main".into(),
        }),
    }
}
