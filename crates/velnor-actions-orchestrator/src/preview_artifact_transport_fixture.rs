//! Pure native-transport fixtures; no source task, process or network access.
use super::super::github::sha256;
use super::*;
use velnor_actions_contract::{
    PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlan, PackageReleasePlanProduct,
    package_release::{PackageReleaseChannel, PackageReleasePayloadFormat},
    preview_production::{
        RELEASE_PRODUCTION_SCHEMA_V1, ReleaseProductionContext, release_plan_sha256,
    },
};

pub(in crate::package_release_publish) fn request() -> ReleaseProductionRequest {
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
    ReleaseProductionRequest {
        schema: RELEASE_PRODUCTION_SCHEMA_V1.into(),
        policy_sha256: "f".repeat(64),
        plan_sha256: release_plan_sha256(&plan).expect("plan"),
        plan,
        context: ReleaseProductionContext {
            repository_id: 101,
            head_repository_id: 101,
            source_sha: "a".repeat(40),
            source_ref: "refs/heads/main".into(),
            workflow_authority_sha: "b".repeat(40),
            run_id: 202,
            run_attempt: 2,
            ci_run_id: 201,
            ci_run_attempt: 1,
            commit_count: 1306,
            version: "0.6.4-preview.1306+aaaaaaa".into(),
        },
        stage: ReleaseProductionStage::Assemble { inputs: Vec::new() },
    }
}

pub(in crate::package_release_publish) fn record() -> ArtifactRecord {
    let request = request();
    let pair =
        velnor_actions_contract::preview_production::expected_release_payload_pairs(&request.plan)
            .expect("pairs")
            .into_iter()
            .next()
            .expect("one pair");
    ArtifactRecord {
        id: 303,
        name: release_payload_artifact_name(&request.plan, &pair, &request.context).expect("name"),
        expired: false,
        size_in_bytes: 16,
        digest: Some(format!("sha256:{}", sha256(b"closed ZIP bytes"))),
        workflow_run: Some(ArtifactSource {
            id: 202,
            repository_id: 101,
            head_repository_id: 101,
            head_sha: "b".repeat(40),
            head_branch: "main".into(),
        }),
    }
}
