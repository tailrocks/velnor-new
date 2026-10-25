use super::*;
use super::{
    assets::{safe_asset_name, signing_workflow_ref},
    verifier::require_policy_authority_tip,
};
use velnor_actions_contract::{
    PackageReleasePlanProduct, package_release::PackageReleasePayloadFormat,
};

const SOURCE_SHA: &str = "a12e73afd75b944a872e8295d61a15f93f5789a2";
const AUTHORITY_SHA: &str = "b12e73afd75b944a872e8295d61a15f93f5789a2";
const HISTORICAL_ADMISSION: &[u8] =
    include_bytes!("../../../.velnor/qualification/homebrew/preview-a-source-admission.json");

fn historical_recovery_input() -> HistoricalPackageReleaseRecoveryInput {
    HistoricalPackageReleaseRecoveryInput {
        original_publisher_job_id: 120_711_604_532,
        run_id: 40_078_964_311,
        run_attempt: 2,
        authority_sha: "e737f4b5eb7c1a0d15ba6595fc260407ad178452".to_owned(),
        recovery_job_id: 120_831_039_984,
        artifact_id: 12_554_588_221,
        artifact_name: "velnor-preview-source-r40078964311-a2".to_owned(),
        admission_size: 34_639,
        admission_sha256: "73eacadf794d40c16e39e6693d09741ce9146cefc867d135f4a1fee023daa972"
            .to_owned(),
        policy_sha256: "37545d865186a6ab25755809d434a991659a4c98ae8119e7a07e50822c08933b"
            .to_owned(),
        admission_bytes: HISTORICAL_ADMISSION.to_vec(),
    }
}

fn plan() -> PackageReleasePlan {
    PackageReleasePlan {
        schema: velnor_actions_contract::PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: "jackin-project/jackin".to_owned(),
        default_branch: "main".to_owned(),
        base_version: "0.6.4".to_owned(),
        products: vec![PackageReleasePlanProduct {
            id: "jackin".to_owned(),
            package: "jackin".to_owned(),
            bins: vec!["jackin".to_owned()],
            targets: vec!["aarch64-apple-darwin".to_owned()],
            format: PackageReleasePayloadFormat::TarGz,
        }],
        companions: Vec::new(),
    }
}

fn jackin_historical_plan() -> PackageReleasePlan {
    PackageReleasePlan {
        schema: velnor_actions_contract::PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: "jackin-project/jackin".to_owned(),
        default_branch: "main".to_owned(),
        base_version: "0.6.4".to_owned(),
        products: vec![
            PackageReleasePlanProduct {
                id: "jackin".to_owned(),
                package: "jackin".to_owned(),
                bins: vec![
                    "jackin".to_owned(),
                    "jackin-role".to_owned(),
                    "jackin-usage-broker".to_owned(),
                ],
                targets: vec![
                    "aarch64-apple-darwin".to_owned(),
                    "aarch64-unknown-linux-gnu".to_owned(),
                    "x86_64-apple-darwin".to_owned(),
                    "x86_64-unknown-linux-gnu".to_owned(),
                ],
                format: PackageReleasePayloadFormat::TarGz,
            },
            PackageReleasePlanProduct {
                id: "jackin-capsule".to_owned(),
                package: "jackin-capsule".to_owned(),
                bins: vec!["jackin-capsule".to_owned()],
                targets: vec![
                    "aarch64-unknown-linux-gnu".to_owned(),
                    "x86_64-unknown-linux-gnu".to_owned(),
                ],
                format: PackageReleasePayloadFormat::TarGz,
            },
        ],
        companions: vec![
            velnor_actions_contract::config::RustBinaryReleaseCompanion {
                id: "capsule-manifest".to_owned(),
                path: "capsule-manifest.json".to_owned(),
                asset_name: "capsule-manifest.json".to_owned(),
            },
        ],
    }
}

#[test]
fn protected_jackin_plan_and_formula_spec_preserve_disabled_stable_sentinel() {
    let plan_bytes = include_bytes!("../../../.velnor/qualification/homebrew/preview-plan.json");
    let protected_plan = velnor_actions_contract::parse_package_release_plan(plan_bytes)
        .expect("protected canonical plan");
    assert_eq!(protected_plan, jackin_historical_plan());
    assert_eq!(
        sha256(&protected_plan.canonical_bytes().expect("canonical plan")),
        "5f4dc66b22cbce0f309ea9ab8c93709774a8890f365e0d61277fb2b0b505d9ae"
    );

    let formula_spec_bytes =
        include_bytes!("../../../.velnor/qualification/homebrew/formula-spec.json");
    let formula_spec: velnor_actions_contract::homebrew_spec::HomebrewFormulaSpec =
        serde_json::from_slice(formula_spec_bytes).expect("protected formula spec");
    formula_spec
        .validate()
        .expect("typed formula spec validation");
    assert_eq!(
        velnor_actions_contract::canonical_json_bytes(&formula_spec)
            .expect("canonical formula spec"),
        formula_spec_bytes
    );
    assert_eq!(
        formula_spec.host.bins,
        jackin_historical_plan().products[0].bins
    );
    assert_eq!(formula_spec.stable_sentinel.date, "2026-10-24");
    assert_eq!(
        formula_spec.stable_sentinel.reason,
        "Stable release is not active"
    );
    let mut stable_plan = protected_plan.clone();
    stable_plan.channel = PackageReleaseChannel::Stable;
    let stable_projection = velnor_actions_contract::homebrew_spec::validate_homebrew_projection(
        &formula_spec,
        &stable_plan,
    )
    .expect("stable sentinel projection");
    let sentinel =
        velnor_actions_contract::homebrew_spec::HomebrewFormulaReleaseLiterals::stable_sentinel(
            &formula_spec,
            &stable_projection,
        )
        .expect("explicit disabled stable sentinel");
    assert_eq!(sentinel.version(), "0.0.0");
    let sentinel_formula =
        velnor_actions_contract::homebrew_spec::render_homebrew_formula(&formula_spec, &sentinel)
            .expect("stable sentinel formula");
    assert!(
        String::from_utf8(sentinel_formula)
            .expect("UTF-8 formula")
            .contains("version \"0.0.0\"")
    );
}

#[test]
fn pinned_a_source_admission_projects_all_protected_identity_fields() {
    let admission_bytes =
        include_bytes!("package_release_public_fixtures/preview-a-source-admission.json");
    let admission = crate::release_policy_admission::project_public_historical_admission(
        admission_bytes,
        "73eacadf794d40c16e39e6693d09741ce9146cefc867d135f4a1fee023daa972",
    )
    .expect("pinned A source-admission fixture parses as data");
    assert_eq!(admission.repository_id, 1_197_700_841);
    assert_eq!(admission.head_repository_id, 1_197_700_841);
    assert_eq!(
        admission.source_sha,
        "41bad147b5ba9db09be99b07248531ce1bd867be"
    );
    assert_eq!(admission.source_ref, "refs/heads/main");
    assert_eq!(
        admission.workflow_authority_sha,
        "e737f4b5eb7c1a0d15ba6595fc260407ad178452"
    );
    assert_eq!(admission.run_id, 40_078_964_311);
    assert_eq!(admission.run_attempt, 2);
    assert_eq!(admission.ci_run_id, 40_036_544_370);
    assert_eq!(admission.ci_run_attempt, 1);
    assert_eq!(admission.commit_count, 1362);
    assert_eq!(admission.version, "0.6.4-preview.1362+41bad14");
    assert_eq!(
        admission.plan_sha256,
        "5f4dc66b22cbce0f309ea9ab8c93709774a8890f365e0d61277fb2b0b505d9ae"
    );
    assert_eq!(
        admission.policy_sha256,
        "37545d865186a6ab25755809d434a991659a4c98ae8119e7a07e50822c08933b"
    );
}

#[test]
fn protected_jackin_a_and_d_catalog_binds_distinct_historical_runs() {
    let a: serde_json::Value = serde_json::from_str(include_str!(
        "../../../.velnor/qualification/homebrew/preview-a.json"
    ))
    .expect("A catalog record");
    let d: serde_json::Value = serde_json::from_str(include_str!(
        "../../../.velnor/qualification/homebrew/preview-d.json"
    ))
    .expect("D catalog record");

    for record in [&a, &d] {
        assert_eq!(record["schema"], "velnor.homebrew-historical-preview.v1");
        assert_eq!(record["repository"]["id"], 1_197_700_841_u64);
        assert_eq!(record["repository"]["slug"], "jackin-project/jackin");
        assert_eq!(
            record["plan_sha256"],
            "5f4dc66b22cbce0f309ea9ab8c93709774a8890f365e0d61277fb2b0b505d9ae"
        );
        assert_eq!(record["evidence"]["profile"], "signed-sbom-v1");
    }

    assert_eq!(a["label"], "A");
    assert_eq!(a["release"]["id"], 422_399_645_u64);
    assert_eq!(a["release"]["version"], "0.6.4-preview.1362+41bad14");
    assert_eq!(a["release"]["commit_count"], 1362_u64);
    assert_eq!(a["release"]["receipt_asset_id"], 670_727_432_u64);
    assert_eq!(a["producer"]["signer_job"]["id"], 120_710_985_043_u64);
    assert_eq!(a["producer"]["publisher_job"]["conclusion"], "cancelled");
    assert_eq!(a["recovery"]["run_id"], 40_078_964_311_u64);
    assert_eq!(a["recovery"]["job"]["id"], 120_831_039_984_u64);
    assert_eq!(
        a["recovery"]["source_admission_artifact"]["sha256"],
        "73eacadf794d40c16e39e6693d09741ce9146cefc867d135f4a1fee023daa972"
    );

    assert_eq!(d["label"], "D");
    assert_eq!(d["release"]["id"], 422_676_197_u64);
    assert_eq!(d["release"]["version"], "0.6.4-preview.1370+5094139");
    assert_eq!(d["release"]["commit_count"], 1370_u64);
    assert_eq!(d["release"]["receipt_asset_id"], 672_027_695_u64);
    assert_eq!(d["producer"]["signer_job"]["id"], 120_854_174_043_u64);
    assert_eq!(d["producer"]["publisher_job"]["id"], 120_854_366_949_u64);
    assert_eq!(d["producer"]["publisher_job"]["conclusion"], "success");
    assert_eq!(d["producer"]["failure_scope"], "downstream-tap-preparation");
}

#[test]
fn trusted_policy_binds_repository_and_protected_authority_revision() {
    let policy = PackageReleasePublicPolicy::from_trusted_source_authority(
        plan(),
        1_197_700_841,
        AUTHORITY_SHA.to_owned(),
    )
    .expect("valid trusted authority");
    assert_eq!(policy.plan.repository, "jackin-project/jackin");
    assert_eq!(policy.repository_id, 1_197_700_841);
    assert_eq!(policy.authority_sha, AUTHORITY_SHA);
    assert_eq!(policy.digest().expect("policy digest").len(), 64);
    assert!(
        PackageReleasePublicPolicy::from_trusted_source_authority(
            plan(),
            0,
            AUTHORITY_SHA.to_owned(),
        )
        .is_err()
    );
}

#[test]
fn historical_admission_binds_plan_release_receipt_and_source() {
    let plan = plan();
    let plan_sha = sha256(&plan.canonical_bytes().expect("canonical plan"));
    let admission = HistoricalPackageReleaseAdmission::new_from_protected_policy(
        &plan,
        SOURCE_SHA.to_owned(),
        format!("preview-{SOURCE_SHA}"),
        422_399_645,
        670_727_432,
        "ec38a14cdbb4f282e9dc79707c31a99b0e2068394e3ddec7f42eb8283620df80".to_owned(),
        plan_sha,
    )
    .expect("pinned historical release");
    let policy = PackageReleasePublicPolicy::from_trusted_source_authority(
        plan.clone(),
        1_197_700_841,
        AUTHORITY_SHA.to_owned(),
    )
    .expect("trusted policy")
    .with_historical_admissions(vec![admission])
    .expect("historical policy");
    assert_eq!(policy.historical.len(), 1);
    assert_eq!(
        policy.digest().expect("history-bound policy digest").len(),
        64
    );
    assert!(
        policy
            .clone()
            .with_historical_admissions(vec![
                policy.historical[0].clone(),
                policy.historical[0].clone()
            ])
            .is_err()
    );
    assert!(
        HistoricalPackageReleaseAdmission::new_from_protected_policy(
            &plan,
            SOURCE_SHA.to_owned(),
            format!("preview-{SOURCE_SHA}"),
            422_399_645,
            670_727_432,
            "ec38a14cdbb4f282e9dc79707c31a99b0e2068394e3ddec7f42eb8283620df80".to_owned(),
            "0".repeat(64),
        )
        .is_err()
    );
}

#[test]
fn historical_a_recovery_requires_its_exact_protected_recovery_pin() {
    let plan = plan();
    let plan_sha = sha256(&plan.canonical_bytes().expect("canonical plan"));
    let admission = HistoricalPackageReleaseAdmission::new_from_protected_policy(
        &plan,
        "41bad147b5ba9db09be99b07248531ce1bd867be".to_owned(),
        "preview-41bad147b5ba9db09be99b07248531ce1bd867be".to_owned(),
        422_399_645,
        670_727_432,
        "ec38a14cdbb4f282e9dc79707c31a99b0e2068394e3ddec7f42eb8283620df80".to_owned(),
        plan_sha,
    )
    .expect("A release pin");
    let recovery =
        HistoricalPackageReleaseRecovery::new_from_protected_policy(historical_recovery_input())
            .expect("recovery identity and retained bytes");
    let admitted = admission
        .with_recovery_admission(recovery.clone())
        .expect("exact A recovery evidence");
    assert_eq!(admitted.recovery, Some(recovery));

    assert!(
        HistoricalPackageReleaseRecovery::new_from_protected_policy(
            HistoricalPackageReleaseRecoveryInput {
                run_attempt: 0,
                ..historical_recovery_input()
            }
        )
        .is_err()
    );

    let mut changed_bytes = historical_recovery_input();
    changed_bytes.admission_bytes[0] ^= 1;
    assert!(HistoricalPackageReleaseRecovery::new_from_protected_policy(changed_bytes).is_err());

    let mut wrong_size = historical_recovery_input();
    wrong_size.admission_size += 1;
    assert!(HistoricalPackageReleaseRecovery::new_from_protected_policy(wrong_size).is_err());
}

#[test]
fn selection_and_asset_names_reject_ambiguous_inputs() {
    assert!(
        PackageReleasePublicSelection::new(
            PackageReleaseChannel::Preview,
            format!("preview-{SOURCE_SHA}"),
            SOURCE_SHA.to_owned(),
        )
        .is_ok()
    );
    assert!(
        PackageReleasePublicSelection::new(
            PackageReleaseChannel::Preview,
            "preview-short".to_owned(),
            "short".to_owned(),
        )
        .is_err()
    );
    for name in [
        "",
        "../release.json",
        "dir/release.json",
        "dir\\release.json",
    ] {
        assert!(!safe_asset_name(name));
    }
    assert!(safe_asset_name("jackin-aarch64-apple-darwin.tar.gz"));
}

#[test]
fn signing_reference_uses_the_full_workflow_reference_from_trusted_fields() {
    let plan = plan();
    let source_ref = "refs/heads/main";
    // Receipt.workflow.workflow_ref is the historical short form. Signing
    // identity must bind repository, workflow path, and source ref instead.
    assert_eq!(
        signing_workflow_ref(&plan.repository, plan.channel.workflow_path(), source_ref),
        "jackin-project/jackin/.github/workflows/binary-preview.yml@refs/heads/main"
    );
}

#[test]
fn public_receipt_must_terminate_at_the_recorded_protected_authority_tip() {
    assert!(require_policy_authority_tip(AUTHORITY_SHA, AUTHORITY_SHA).is_ok());
    assert!(require_policy_authority_tip(SOURCE_SHA, AUTHORITY_SHA).is_err());
    assert!(require_policy_authority_tip("not-a-sha", AUTHORITY_SHA).is_err());
}

#[path = "package_release_public_asset_tests.rs"]
mod asset_tests;
