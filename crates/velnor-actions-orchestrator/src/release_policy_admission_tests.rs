use super::*;
use std::collections::BTreeSet;
use velnor_actions_contract::{
    PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlan, PackageReleasePlanProduct,
    package_release::{PackageReleaseChannel, PackageReleasePayloadFormat},
    preview_production::ReleaseProductionContext,
};

#[test]
fn historical_admission_projection_hashes_before_strict_parse_and_keeps_config_opaque() {
    let producer: velnor_actions_contract::config::RustBinaryReleaseProducer =
        serde_json::from_value(serde_json::json!({})).expect("default producer");
    let admission = Admission {
        schema: "velnor.preview-source-admission.v1".to_owned(),
        policy_sha256: "1".repeat(64),
        plan_sha256: "2".repeat(64),
        context: ReleaseProductionContext {
            repository_id: 1_197_700_841,
            head_repository_id: 1_197_700_841,
            source_sha: "41bad147b5ba9db09be99b07248531ce1bd867be".to_owned(),
            source_ref: "refs/heads/main".to_owned(),
            workflow_authority_sha: "e737f4b5eb7c1a0d15ba6595fc260407ad178452".to_owned(),
            run_id: 40_078_964_311,
            run_attempt: 2,
            ci_run_id: 40_036_544_370,
            ci_run_attempt: 1,
            commit_count: 1362,
            version: "0.6.4-preview.1362+41bad14".to_owned(),
        },
        source_files: Vec::new(),
        producer,
        task_config: "this is deliberately not parsed as configuration".to_owned(),
        task_inputs: Vec::new(),
        tool_specs: Vec::new(),
        qualified_tools: Vec::new(),
        qualification_digest: String::new(),
    };
    let bytes = velnor_actions_contract::canonical_json_bytes(&admission).expect("canonical");
    let digest = sha256(&bytes);
    let projection =
        project_public_historical_admission(&bytes, &digest).expect("typed data-only projection");
    assert_eq!(projection.source_sha, admission.context.source_sha);
    assert_eq!(
        projection.workflow_authority_sha,
        admission.context.workflow_authority_sha
    );
    assert_eq!(projection.plan_sha256, admission.plan_sha256);
    assert!(project_public_historical_admission(&bytes, &"0".repeat(64)).is_err());
}

fn plan() -> PackageReleasePlan {
    PackageReleasePlan {
        schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
        channel: PackageReleaseChannel::Preview,
        repository: "jackin-project/jackin".to_owned(),
        default_branch: "main".to_owned(),
        base_version: "0.4.0".to_owned(),
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

#[test]
fn retained_multi_member_manifest_closure_covers_selected_policy_files() {
    let root_manifest = br#"[workspace]
members = ["crates/*"]
exclude = ["crates/ignored"]
resolver = "3"

[workspace.package]
version = "0.4.0"
"#;
    let regular_paths = [
        "Cargo.toml",
        "crates/jackin/Cargo.toml",
        "crates/jackin-cli/Cargo.toml",
        "crates/ignored/Cargo.toml",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<std::collections::BTreeSet<_>>();
    let manifests = velnor_actions_rust::release_workspace_manifest_paths(
        "Cargo.toml",
        root_manifest,
        &regular_paths,
    )
    .expect("all authenticated workspace member manifests");
    assert_eq!(
        manifests,
        [
            "Cargo.toml".to_owned(),
            "crates/jackin-cli/Cargo.toml".to_owned(),
            "crates/jackin/Cargo.toml".to_owned()
        ]
    );
    let mut retained = manifests
        .into_iter()
        .chain([
            ".velnor/config.toml".to_owned(),
            ".mise/tasks/release.toml".to_owned(),
        ])
        .map(|path| source_files::SourceFileEntry {
            path: path.clone(),
            mode: "100644".to_owned(),
            sha: "a".repeat(40),
        })
        .collect::<Vec<_>>();
    retained.sort_by(|left, right| left.path.cmp(&right.path));
    let actual = admission_paths(&retained).expect("unique retained closure");
    let required = [
        "Cargo.toml",
        "crates/jackin/Cargo.toml",
        ".velnor/config.toml",
        ".mise/tasks/release.toml",
    ];
    let required = required.into_iter().map(str::to_owned).collect();
    assert!(admission_covers_required_paths(&required, &actual));
    assert!(actual.contains("crates/jackin-cli/Cargo.toml"));

    let missing = BTreeSet::from(["Cargo.toml".to_owned()]);
    assert!(!admission_covers_required_paths(&required, &missing));

    let mut duplicate = retained;
    duplicate.push(duplicate[1].clone());
    assert!(admission_paths(&duplicate).is_err());
}

#[test]
fn admitted_preview_version_uses_source_a_identity_not_base_version() {
    let plan = plan();
    let source = "a".repeat(40);
    let authority = "b".repeat(40);
    let identity = velnor_actions_contract::preview_identity::PreviewIdentity::derive(
        &plan.base_version,
        &source,
        17,
    )
    .expect("preview identity");
    let context = ReleaseProductionContext {
        repository_id: 1,
        head_repository_id: 1,
        source_sha: source,
        source_ref: "refs/heads/main".to_owned(),
        workflow_authority_sha: authority,
        run_id: 10,
        run_attempt: 1,
        ci_run_id: 11,
        ci_run_attempt: 1,
        commit_count: 17,
        version: identity.version.clone(),
    };
    assert_ne!(context.version, plan.base_version);
    assert_ne!(context.source_sha, context.workflow_authority_sha);
    assert!(admitted_preview_version_matches(&plan, &context));

    let mut wrong_source_version = context;
    wrong_source_version.version = plan.base_version.clone();
    assert!(!admitted_preview_version_matches(
        &plan,
        &wrong_source_version
    ));

    let mut wrong_source = wrong_source_version;
    wrong_source.source_sha = wrong_source.workflow_authority_sha.clone();
    assert!(!admitted_preview_version_matches(&plan, &wrong_source));
}
