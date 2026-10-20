//! Independent six-pair request fixtures, not a reduced package plan.

use super::*;
use crate::package_release::{PackageReleaseChannel, PackageReleasePayloadFormat};
use crate::{PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlanProduct};

pub(super) fn request() -> ReleaseProductionRequest {
    let plan = PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: "jackin-project/jackin".to_owned(),
        default_branch: "main".to_owned(),
        base_version: "0.6.4".to_owned(),
        products: vec![
            product(
                "jackin",
                &["jackin", "jackin-role", "jackin-usage-broker"],
                &[
                    "aarch64-apple-darwin",
                    "aarch64-unknown-linux-gnu",
                    "x86_64-apple-darwin",
                    "x86_64-unknown-linux-gnu",
                ],
            ),
            product(
                "jackin-capsule",
                &["jackin-capsule"],
                &["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"],
            ),
        ],
        companions: Vec::new(),
    };
    ReleaseProductionRequest {
        schema: RELEASE_PRODUCTION_SCHEMA_V1.to_owned(),
        plan_sha256: release_plan_sha256(&plan).expect("full plan digest"),
        plan,
        context: ReleaseProductionContext {
            repository_id: 101,
            head_repository_id: 101,
            source_sha: "a".repeat(40),
            source_ref: "refs/heads/main".to_owned(),
            workflow_authority_sha: "a".repeat(40),
            run_id: 202,
            run_attempt: 2,
            ci_run_id: 201,
            ci_run_attempt: 1,
            commit_count: 1306,
            version: "0.6.4-preview.1306+aaaaaaa".to_owned(),
        },
        stage: ReleaseProductionStage::Payload {
            pair: ReleasePayloadPair {
                product_id: "jackin".to_owned(),
                target: "aarch64-apple-darwin".to_owned(),
            },
        },
    }
}

fn product(id: &str, bins: &[&str], targets: &[&str]) -> PackageReleasePlanProduct {
    PackageReleasePlanProduct {
        id: id.to_owned(),
        package: id.to_owned(),
        bins: bins.iter().map(|value| (*value).to_owned()).collect(),
        targets: targets.iter().map(|value| (*value).to_owned()).collect(),
        format: PackageReleasePayloadFormat::TarGz,
    }
}

pub(super) fn assembly() -> ReleaseProductionRequest {
    let mut request = request();
    request.stage = ReleaseProductionStage::Assemble {
        inputs: expected_release_payload_pairs(&request.plan)
            .expect("pairs")
            .into_iter()
            .enumerate()
            .map(|(index, pair)| PayloadArtifactRef {
                pair,
                artifact_id: u64::try_from(index).expect("small index") + 100,
                sha256: "b".repeat(64),
            })
            .collect(),
    };
    request
}
