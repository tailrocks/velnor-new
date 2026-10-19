use super::*;
use crate::{HelperProvenance, provision_acquire_step};
use velnor_actions_contract::preview_production::{
    expected_preview_payload_pairs, preview_payload_job_id,
};
use velnor_actions_contract::{
    ExecutionMode, PACKAGE_RELEASE_PLAN_SCHEMA_V1, PackageReleasePlanProduct, ScaleSetSelector,
    config::{RustBinaryPreviewCompanion, VerificationRunner},
    package_release::PackageReleasePayloadFormat,
};

fn hosted_policy() -> crate::workflow_routing::WorkflowRoutingPolicy {
    crate::workflow_routing::test_policy(ExecutionMode::Hosted, scale_set_selector())
}

fn scale_set_policy() -> crate::workflow_routing::WorkflowRoutingPolicy {
    crate::workflow_routing::test_policy(ExecutionMode::ScaleSet, scale_set_selector())
}

fn scale_set_selector() -> ScaleSetSelector {
    ScaleSetSelector::try_new(
        "ubuntu-26.04-scale-set",
        &["velnor".to_owned(), "ubuntu-26.04-scale-set".to_owned()],
    )
    .expect("valid selector")
}

fn render_preview(spec: &ConsumerPreviewSpec) -> Result<RenderedFile, RenderError> {
    render_consumer_preview(spec, &hosted_policy())
}
#[path = "schema2_consumer_preview_payload_tests.rs"]
mod payloads;
#[path = "schema2_consumer_preview_reconcile_tests.rs"]
mod reconcile_tests;
#[path = "schema2_consumer_preview_source_tests.rs"]
mod sources;
#[path = "schema2_consumer_preview_tap_admission_tests.rs"]
mod tap_admission;

fn fixture() -> ConsumerPreviewSpec {
    let acquire = provision_acquire_step(&HelperProvenance::ReleaseAsset {
        url: "https://github.com/tailrocks/velnor-new/releases/download/v0.1.12/velnor-actions-0.1.12-x86_64-unknown-linux-gnu".to_owned(),
        sha256: "a".repeat(64), commit: "b".repeat(40),
    }, vec!["sh".to_owned(), "-c".to_owned(), r#"curl -fsSL "$VELNOR_ASSET_URL" -o "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.12" && printf '%s  %s\n' "$VELNOR_ASSET_SHA256" "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.12" | sha256sum -c -"#.to_owned()]).expect("pinned helper");
    let targets = [
        "aarch64-apple-darwin",
        "aarch64-unknown-linux-gnu",
        "x86_64-apple-darwin",
        "x86_64-unknown-linux-gnu",
    ];
    let products = [("capsule", &targets[..2]), ("scanner", &targets[..])]
        .into_iter()
        .map(|(id, targets)| PackageReleasePlanProduct {
            id: id.to_owned(),
            package: id.to_owned(),
            bins: vec![id.to_owned()],
            targets: targets.iter().map(|target| (*target).to_owned()).collect(),
            format: PackageReleasePayloadFormat::TarGz,
        })
        .collect();
    ConsumerPreviewSpec {
        generator_version: "0.1.12".to_owned(),
        plan: PackageReleasePlan {
            schema: PACKAGE_RELEASE_PLAN_SCHEMA_V1.to_owned(),
            channel: PackageReleaseChannel::Preview,
            repository: "example/scanner".to_owned(),
            default_branch: "main".to_owned(),
            base_version: "1.2.3".to_owned(),
            products,
            companions: vec![RustBinaryPreviewCompanion {
                id: "manifest".to_owned(),
                path: "capsule.json".to_owned(),
                asset_name: "capsule.json".to_owned(),
            }],
        },
        producer_runner: VerificationRunner::Macos26Arm64,
        producer_timeout_minutes: 5,
        linux_acquire: acquire.clone(),
        producer_acquire: acquire,
        producer_setup: setup_fixture(),
        linux_setup: setup_fixture(),
        install_gh_argv: vec![
            "mise".to_owned(),
            "--no-config".to_owned(),
            "install".to_owned(),
            "gh@2.102.0".to_owned(),
        ],
        install_rust_argv: vec![
            "mise".to_owned(),
            "--no-config".to_owned(),
            "install".to_owned(),
            "rust@1.98.1".to_owned(),
        ],
        producer_source_prep: Vec::new(),
        producer_cargo_env: BTreeMap::from([
            (
                "MISE_CARGO_HOME".to_owned(),
                "${{ runner.temp }}/velnor/cargo".to_owned(),
            ),
            (
                "MISE_RUSTUP_HOME".to_owned(),
                "${{ runner.temp }}/velnor/rustup".to_owned(),
            ),
            (
                "CARGO_HOME".to_owned(),
                "${{ runner.temp }}/velnor/cargo".to_owned(),
            ),
            (
                "RUSTUP_HOME".to_owned(),
                "${{ runner.temp }}/velnor/rustup".to_owned(),
            ),
            ("RUSTUP_TOOLCHAIN".to_owned(), "1.98.1".to_owned()),
        ]),
        gh_argv: vec!["gh".to_owned()],
        homebrew_tap: None,
    }
}

fn setup_fixture() -> MiseSetup {
    MiseSetup {
        uses: "jdx/mise-action@9b200bcecbfd6303e69a4a261b2e09e3757440a7".to_owned(),
        version: "2026.10.14".to_owned(),
        sha256: "a".repeat(64),
    }
}

fn job<'a>(yaml: &'a str, name: &str, next: Option<&str>) -> &'a str {
    let start = yaml.find(&format!("  {name}:\n")).expect("job exists");
    let rest = &yaml[start..];
    next.map_or(rest, |next| {
        &rest[..rest.find(&format!("  {next}:\n")).expect("next job")]
    })
}

fn payload_job_ids(spec: &ConsumerPreviewSpec) -> Vec<String> {
    expected_preview_payload_pairs(&spec.plan)
        .expect("valid payload pairs")
        .iter()
        .map(|pair| preview_payload_job_id(&spec.plan, pair).expect("payload job ID"))
        .collect()
}

fn workflow_job_ids(yaml: &str) -> Vec<&str> {
    let jobs = yaml.split_once("jobs:\n").map_or("", |(_, tail)| tail);
    jobs.lines()
        .filter_map(|line| {
            let field = line.strip_prefix("  ")?;
            if field.starts_with(' ') {
                return None;
            }
            field.strip_suffix(':')
        })
        .collect()
}

#[test]
fn preview_rejects_stable_plan_or_invalid_inventory_before_rendering() {
    let mut spec = fixture();
    spec.plan.channel = PackageReleaseChannel::Stable;
    assert!(render_preview(&spec).is_err());
    spec.plan.channel = PackageReleaseChannel::Preview;
    spec.plan.default_branch = "develop".to_owned();
    assert!(render_preview(&spec).is_err());
    spec.plan.default_branch = "main".to_owned();
    spec.plan.products[0]
        .targets
        .push("../../escape".to_owned());
    assert!(render_preview(&spec).is_err());
}

fn with_tap() -> ConsumerPreviewSpec {
    use velnor_actions_contract::config::{
        CheckExecutor, CheckPlatform, CheckRunner, ExactHeadReviewCheck, HomebrewTapConfig,
        HomebrewTapProtectionObserver, HomebrewTapSource, MiseTaskSource, ReviewAppIdentity,
        TapMergePolicy, TapUpdateEnvironment, TapUpdaterBinding,
    };
    let mut spec = fixture();
    spec.homebrew_tap = Some(ConsumerPreviewTapSpec {
        updater_acquire: spec.linux_acquire.clone(),
        updater_setup: setup_fixture(),
        policy: HomebrewTapConfig {
            enabled: true,
            source: HomebrewTapSource::RustBinaryReleasePreview,
            repository: "example/homebrew-tap".to_owned(),
            formula: "Formula/scanner-preview.rb".to_owned(),
            updater: TapUpdaterBinding {
                task: "package:update-preview".to_owned(),
                source: MiseTaskSource {
                    mise_config: "mise.toml".to_owned(),
                    working_directory: ".".to_owned(),
                },
                runner: CheckRunner {
                    label: "ubuntu-26.04".to_owned(),
                    platform: CheckPlatform::LinuxX64,
                    executor: CheckExecutor::Hosted,
                    container: None,
                },
                inputs: vec![
                    "mise.toml".to_owned(),
                    "Formula/scanner-preview.rb".to_owned(),
                ],
                tools: vec!["python".to_owned()],
                timeout_minutes: 5,
            },
            environment: TapUpdateEnvironment::TapUpdate,
            app_id_variable: "TAP_APP_ID".to_owned(),
            app_private_key_secret: "TAP_APP_PRIVATE_KEY".to_owned(),
            protection_observer: HomebrewTapProtectionObserver {
                app_id_variable: "PROTECTION_APP_ID".to_owned(),
                app_private_key_secret: "PROTECTION_APP_PRIVATE_KEY".to_owned(),
            },
            merge: TapMergePolicy::SquashAfterGates,
            review_completion: ExactHeadReviewCheck {
                check_name: "Review completion".to_owned(),
                app_id_variable: "COMPLETION_APP_ID".to_owned(),
                expected_reviewers: vec![ReviewAppIdentity {
                    app_id_variable: "REVIEWER_APP_ID".to_owned(),
                }],
            },
        },
    });
    spec
}

#[test]
fn tap_jobs_separate_unprivileged_preparation_and_scoped_app_authority() {
    let rendered = render_preview(&with_tap()).expect("tap workflow");
    let yaml = &rendered.bytes;
    let prepare = job(yaml, "preview-tap-prepare", Some("preview-tap-writer"));
    assert!(prepare.contains("actions/checkout"));
    assert!(!prepare.contains("environment:"));
    assert!(!prepare.contains("create-github-app-token"));
    assert!(!prepare.contains("${{ secrets.TAP_APP_PRIVATE_KEY }}"));
    assert!(!prepare.contains("\n      contents: write\n"));
    assert!(prepare.contains("needs.preview-recovery.result == 'success'"));
    assert!(prepare.contains("needs.preview-publisher.result == 'success'"));
    for (id, next) in [
        ("preview-tap-writer", Some("preview-tap-merge")),
        ("preview-tap-merge", None),
    ] {
        let privileged = job(yaml, id, next);
        assert!(!privileged.contains("actions/checkout"));
        assert!(!privileged.contains("id-token:"));
        assert!(!privileged.contains("\n      contents: write\n"));
        assert!(privileged.contains("environment: tap-update"));
        assert!(privileged.contains("owner: example"));
        assert!(privileged.contains("repositories: homebrew-tap"));
        assert!(privileged.contains("skip-token-revoke: false"));
        assert!(!privileged.contains("PREVIEW_IMMUTABILITY_READ_TOKEN"));
        assert!(privileged.contains("VELNOR_RELEASE_ORIGINAL_RUN_ID:"));
    }
    let writer = job(yaml, "preview-tap-writer", Some("preview-tap-merge"));
    assert!(!writer.contains("permission-administration"));
    assert!(writer.contains("VELNOR_TAP_TOKEN:"));
    assert!(!writer.contains("VELNOR_TAP_MERGE_TOKEN:"));
    let merge = job(yaml, "preview-tap-merge", None);
    assert!(merge.contains("permission-administration: read"));
    assert!(merge.contains("permission-checks: read"));
    assert!(merge.contains("VELNOR_TAP_MERGE_TOKEN:"));
    assert!(!merge.contains("VELNOR_TAP_TOKEN:"));
    assert!(merge.contains("VELNOR_TAP_COMPLETION_APP_ID:"));
    assert!(merge.contains("VELNOR_TAP_REVIEWER_APP_ID_0:"));
}

#[test]
fn credential_references_cannot_become_privileged_environment_keys() {
    let mut spec = with_tap();
    spec.homebrew_tap
        .as_mut()
        .expect("tap")
        .policy
        .review_completion
        .app_id_variable = "PATH".to_owned();
    let yaml = render_preview(&spec)
        .expect("literal variable reference")
        .bytes;
    let merge = job(&yaml, "preview-tap-merge", None);
    assert!(merge.contains("VELNOR_TAP_COMPLETION_APP_ID: ${{ vars.PATH }}"));
    assert!(!merge.contains("\n          PATH:"));
}

#[test]
fn tap_updater_rejects_self_hosted_execution_without_native_provisioning() {
    let mut spec = with_tap();
    let runner = &mut spec
        .homebrew_tap
        .as_mut()
        .expect("tap")
        .policy
        .updater
        .runner;
    runner.executor = velnor_actions_contract::config::CheckExecutor::EphemeralSelfHosted;
    runner.label = "tap-updater".to_owned();
    assert!(render_preview(&spec).is_err());
}

#[test]
fn scale_set_routes_every_preview_and_tap_job_into_pinned_containers() {
    let mut spec = with_tap();
    spec.producer_runner = VerificationRunner::LinuxX64;
    let hosted = render_preview(&spec).expect("hosted preview");
    let scale_set = render_consumer_preview(&spec, &scale_set_policy()).expect("Scale Set preview");
    let selector = "runs-on: [velnor, ubuntu-26.04-scale-set]";
    let job_ids = workflow_job_ids(&scale_set.bytes);
    assert_ne!(job_ids.len(), 0);
    assert_eq!(scale_set.bytes.matches(selector).count(), job_ids.len());
    let image = "container:\n      image: ghcr.io/tailrocks/velnor-job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    assert_eq!(scale_set.bytes.matches(image).count(), job_ids.len());
    assert!(!hosted.bytes.contains(image));
    crate::workflow_routing::validate_generated_workflow_calls(
        std::slice::from_ref(&scale_set),
        &scale_set_policy(),
    )
    .expect("parsed final bytes inspect every routed preview and tap job");
}

#[test]
fn scale_set_rejects_the_native_preview_producer_with_a_specific_error() {
    let spec = fixture();
    let first_producer = payload_job_ids(&spec)[0].clone();
    let error = render_consumer_preview(&spec, &scale_set_policy())
        .expect_err("a Linux-only Scale Set cannot run a Darwin producer");
    assert_eq!(
        error,
        RenderError::InvalidWorkflow(format!(
            "unsupported_darwin_workload:.github/workflows/binary-preview.yml:{first_producer}:macos-26"
        ))
    );
}

#[test]
fn preview_rejects_unbounded_producer_limits_before_rendering() {
    for minutes in [0, 361, u16::MAX] {
        let mut spec = fixture();
        spec.producer_timeout_minutes = minutes;
        assert!(
            render_consumer_preview(
                &spec,
                &crate::workflow_routing::WorkflowRoutingPolicy::hosted_only()
            )
            .is_err(),
            "{minutes}"
        );
    }
}

#[path = "schema2_consumer_preview_trigger_tests.rs"]
mod triggers;

#[path = "schema2_consumer_preview_privilege_tests.rs"]
mod privilege_tests;
