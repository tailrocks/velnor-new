//! Opt-in dirty-tree diagnostic: inspect an actual closed Jackin producer handoff.
use super::*;
use std::io::Write as _;
use velnor_actions_contract::{PackageReleasePlan, parse_package_release_plan};

fn required(name: &str) -> String {
    std::env::var(name).expect("explicit real-artifact proof input")
}

#[test]
#[ignore = "requires a finished diagnostic producer, independent trusted plan/source/version, and fresh staging"]
fn inspect_real_jackin_preview_handoff() {
    let producer = PathBuf::from(required("VELNOR_REAL_PACKAGE_OUTPUT"));
    let staging = PathBuf::from(required("VELNOR_REAL_PACKAGE_STAGING"));
    let plan_path = PathBuf::from(required("VELNOR_REAL_PACKAGE_PLAN"));
    let report_path = PathBuf::from(required("VELNOR_REAL_PACKAGE_REPORT"));
    for path in [&producer, &staging, &plan_path, &report_path] {
        assert!(path.is_absolute(), "proof inputs must be absolute");
    }
    assert!(!staging.exists(), "verifier owns a fresh staging directory");
    assert!(
        !report_path.exists(),
        "preserve previous qualification reports"
    );
    let source_commit = required("VELNOR_REAL_PACKAGE_SOURCE");
    let version = required("VELNOR_REAL_PACKAGE_VERSION");
    let plan = parse_package_release_plan(&fs::read(plan_path).expect("independent policy"))
        .expect("canonical trusted plan");
    validate_jackin_policy(&plan);
    let verified = verify_package_output(
        &producer,
        "package-output.json",
        &staging,
        &plan.products,
        &plan.companions,
        PackageOutputIdentity {
            source_commit: &source_commit,
            source_ref: "refs/heads/main",
            version: &version,
        },
    )
    .expect("independent native admission of actual six-payload producer output");
    assert_eq!(verified.source_commit, source_commit);
    assert_eq!(verified.source_ref, "refs/heads/main");
    assert_eq!(verified.version, version);
    assert_eq!(
        verified.artifacts.len(),
        13,
        "six payloads, six SBOMs, one companion"
    );
    assert_eq!(
        fs::read_dir(&staging).expect("staged inventory").count(),
        13
    );
    let artifacts: Vec<_> = verified
        .artifacts
        .iter()
        .map(|artifact| {
            serde_json::json!({
                "name":artifact.name, "size":artifact.size, "sha256":artifact.sha256,
                "release_asset":artifact.release_asset,
            })
        })
        .collect();
    let report = serde_json::json!({
        "schema":"jackin.diagnostic-native-producer-inspection.v1",
        "evidence_kind":"diagnostic_dirty_tree_producer_handoff",
        "publication_proof":false, "expected_source_commit":source_commit,
        "synthetic_required_source_ref":"refs/heads/main", "expected_version":version,
        "trusted_plan":plan, "verified_artifacts":artifacts,
    });
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&report_path)
        .expect("new diagnostic report")
        .write_all(&serde_json::to_vec_pretty(&report).expect("report JSON"))
        .expect("report bytes");
    println!(
        "Diagnostic native inspection passed: six archives, six SBOMs, one companion; report {}",
        report_path.display()
    );
}

fn validate_jackin_policy(plan: &PackageReleasePlan) {
    use velnor_actions_contract::package_release::{
        PackageReleaseChannel, PackageReleasePayloadFormat,
    };
    assert_eq!(plan.repository, "jackin-project/jackin");
    assert_eq!(plan.default_branch, "main");
    assert_eq!(plan.base_version, "0.6.4");
    assert_eq!(plan.channel, PackageReleaseChannel::Preview);
    assert_eq!(plan.products.len(), 2);
    assert_eq!(plan.products[0].id, "jackin");
    assert_eq!(plan.products[0].package, "jackin");
    assert_eq!(
        plan.products[0].bins,
        ["jackin", "jackin-role", "jackin-usage-broker"]
    );
    assert_eq!(
        plan.products[0].targets,
        [
            "aarch64-apple-darwin",
            "aarch64-unknown-linux-gnu",
            "x86_64-apple-darwin",
            "x86_64-unknown-linux-gnu"
        ]
    );
    assert_eq!(plan.products[1].id, "jackin-capsule");
    assert_eq!(plan.products[1].package, "jackin-capsule");
    assert_eq!(plan.products[1].bins, ["jackin-capsule"]);
    assert_eq!(
        plan.products[1].targets,
        ["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"]
    );
    assert!(
        plan.products
            .iter()
            .all(|product| product.format == PackageReleasePayloadFormat::TarGz)
    );
    assert_eq!(
        plan.companions,
        [RustBinaryPreviewCompanion {
            id: "capsule-manifest".into(),
            path: "capsule-manifest.json".into(),
            asset_name: "capsule-manifest.json".into(),
        }]
    );
}
