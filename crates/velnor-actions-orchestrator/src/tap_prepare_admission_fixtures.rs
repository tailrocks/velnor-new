//! Complete synthetic authenticated-handoff semantic witnesses.
use super::*;
use crate::tap_prepare::sha256;
use serde_json::json;
use velnor_actions_contract::{
    CargoVersionBinding, CargoVersionPolicy, PACKAGE_RELEASE_PLAN_SCHEMA_V1,
    PACKAGE_RELEASE_POLICY_SCHEMA_V1, PackageReleasePlanProduct, PackageReleasePolicy,
    package_release::{
        PACKAGE_RELEASE_EVIDENCE_PROFILE, PACKAGE_RELEASE_SCHEMA_V2, PackageReleaseChannel,
        PackageReleasePayloadFormat,
    },
};
const PAYLOAD: &str = "app-aarch64-apple-darwin.tar.gz";
const BODY: &[u8] = b"asset-body";
pub(super) fn plan() -> PackageReleasePlan {
    PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.into(),
        channel: PackageReleaseChannel::Preview,
        repository: "example/app".into(),
        default_branch: "main".into(),
        base_version: "1.2.3".into(),
        products: vec![PackageReleasePlanProduct {
            id: "app".into(),
            package: "app".into(),
            bins: vec!["app".into()],
            targets: vec!["aarch64-apple-darwin".into()],
            format: PackageReleasePayloadFormat::TarGz,
        }],
        companions: vec![],
    }
}
pub(super) fn receipt() -> PackageReleaseReceipt {
    let channel = PackageReleaseChannel::Preview;
    let mut assets = [
        (PAYLOAD.to_owned(), "payload"),
        (format!("{PAYLOAD}.sha256"), "sha256"),
        (format!("{PAYLOAD}.bundle"), "signature_bundle"),
        (format!("{PAYLOAD}.sbom.json"), "sbom"),
        ("SHA256SUMS".into(), "checksum_list"),
    ]
    .into_iter()
    .map(|(name, kind)| {
        json!({"name":name,"kind":kind,
        "product_id":if kind=="checksum_list" {None} else {Some("app")},
        "target":if kind=="checksum_list" {None} else {Some("aarch64-apple-darwin")},
        "subject_name":if kind=="checksum_list" {None} else {Some(PAYLOAD)},
        "size":BODY.len(),"sha256":sha256(BODY)})
    })
    .collect::<Vec<_>>();
    assets.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    let receipt: PackageReleaseReceipt = serde_json::from_value(json!({
        "schema":PACKAGE_RELEASE_SCHEMA_V2,"channel":"preview", "repository":{"slug":"example/app","id":123},
        "source":{"commit_sha":"a".repeat(40),"ref":"refs/heads/main","default_branch":"main",
            "event":"workflow_run","ci_run_id":41,"ci_run_attempt":1},
        "workflow":{"path":channel.workflow_path(),"ref":"refs/heads/main","authority_sha":"a".repeat(40),
            "run_id":42,"run_attempt":2,"producer_run_id":42,"producer_run_attempt":2,
            "signer_job_id":channel.signer_job_id(),"signer_workflow":channel.workflow_path()},
        "version":{"base":"1.2.3","value":"1.2.3-preview.9+aaaaaaa","commit_count":9,"source_sha7":"aaaaaaa"},
        "tag":format!("preview-{}", "a".repeat(40)),
        "products":[{"id":"app","package":"app","package_version":"1.2.3","bins":["app"],
            "payloads":[{"format":"tar-gz","target":"aarch64-apple-darwin","name":PAYLOAD,"members":["app"]}]}],
        "companions":[],"assets":assets,
        "evidence":{"profile":PACKAGE_RELEASE_EVIDENCE_PROFILE,"sbom_format":"CycloneDX-JSON",
            "signer_issuer":"https://token.actions.githubusercontent.com",
            "signer_workflow_ref":"example/app/.github/workflows/binary-preview.yml@refs/heads/main",
            "attestation_predicate":"https://slsa.dev/provenance/v1","attested_subject_names":[PAYLOAD,"release.json"]}
    })).expect("typed receipt");
    receipt
        .validate_against_plan(&plan())
        .expect("complete valid signed inventory");
    receipt
}

pub(super) fn policy() -> HomebrewTapConfig {
    serde_json::from_value(json!({"enabled":true,"destinations":{"preview":{"formula":"Formula/app-preview.rb"},"stable":{"formula":"Formula/app.rb"}},"repository":"example/homebrew-tap","environment":"tap-update","app_id_variable":"TAP_APP_ID","app_private_key_secret":"TAP_APP_PRIVATE_KEY","merge":"squash-after-gates","protection_observer":{"app_id_variable":"OBSERVER_APP_ID","app_private_key_secret":"OBSERVER_APP_PRIVATE_KEY"},"review_completion":{"check_name":"Review completion","app_id_variable":"COMPLETION_APP_ID","expected_reviewers":[{"app_id_variable":"REVIEWER_APP_ID"}]},"updater":{"task":"update","source":{"mise_config":"mise.toml","working_directory":"."},"runner":{"label":"ubuntu-26.04","platform":"linux_x64","executor":"hosted"},"inputs":["Formula/app-preview.rb","Formula/app.rb","mise.toml"],"tools":["python"],"timeout_minutes":5}})).expect("policy")
}

pub(super) fn release_policy() -> PackageReleasePolicy {
    PackageReleasePolicy {
        schema: PACKAGE_RELEASE_POLICY_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: "example/app".to_owned(),
        default_branch: "main".to_owned(),
        version: CargoVersionPolicy {
            version_package: "app".to_owned(),
            bindings: vec![CargoVersionBinding {
                package: "app".to_owned(),
                manifest_path: "Cargo.toml".to_owned(),
                workspace_manifest_path: "Cargo.toml".to_owned(),
            }],
        },
        products: plan().products,
        companions: Vec::new(),
    }
}
