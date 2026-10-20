//! Compose real renderers with the runtime's closed role selection.
use super::*;
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CargoVersionBinding, CargoVersionPolicy, PACKAGE_RELEASE_POLICY_SCHEMA_V1,
    PackageReleasePolicy, VerificationRunner,
};
use velnor_actions_workflow_renderer::{
    HelperProvenance, MiseSetup, provision_acquire_step,
    schema2::{
        ConsumerReleaseSpec, ConsumerReleaseTapSpec, ConsumerRollingPreviewSpec,
        ConsumerStableReleaseSpec, render_consumer_binary_release, render_consumer_preview,
        render_release_tap_reconcile,
    },
    workflow_routing::WorkflowRoutingPolicy,
};

fn fixture(channel: PackageReleaseChannel) -> ConsumerReleaseSpec {
    let acquire=provision_acquire_step(&HelperProvenance::ReleaseAsset {
        url:"https://github.com/tailrocks/velnor-new/releases/download/v0.1.30/velnor-actions-0.1.30-x86_64-unknown-linux-gnu".to_owned(),sha256:"a".repeat(64),commit:"b".repeat(40)
    },vec!["sh".to_owned(),"-c".to_owned(),r#"curl -fsSL "$VELNOR_ASSET_URL" -o "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.30" && printf '%s  %s\n' "$VELNOR_ASSET_SHA256" "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.30" | sha256sum -c -"#.to_owned()]).expect("typed acquisition");
    let setup = MiseSetup {
        uses: "jdx/mise-action@fc42bcc7ddbf51124c30f95a877a0c5168a3bfde".to_owned(),
        version: "2026.10.15".to_owned(),
        sha256: "a".repeat(64),
    };
    ConsumerReleaseSpec {
        generator_version: "0.1.30".to_owned(),
        checkout_lfs: false,
        policy: policy(channel),
        producer_runner: VerificationRunner::Macos26Arm64,
        producer_timeout_minutes: 20,
        linux_acquire: acquire.clone(),
        producer_acquire: acquire.clone(),
        producer_setup: setup.clone(),
        linux_setup: setup.clone(),
        install_gh_argv: vec![
            "mise".to_owned(),
            "install".to_owned(),
            "gh@2.102.0".to_owned(),
        ],
        install_rust_argv: vec![
            "mise".to_owned(),
            "install".to_owned(),
            "rust@1.99.1".to_owned(),
        ],
        producer_source_prep: vec![],
        producer_cargo_env: [
            "CARGO_HOME",
            "MISE_CARGO_HOME",
            "RUSTUP_HOME",
            "MISE_RUSTUP_HOME",
        ]
        .into_iter()
        .map(|k| (k.to_owned(), "${{ runner.temp }}/velnor/cargo".to_owned()))
        .collect(),
        gh_argv: vec!["gh".to_owned()],
        homebrew_tap: Some(ConsumerReleaseTapSpec {
            policy: tap_policy(),
            updater_acquire: acquire,
            updater_setup: setup,
        }),
        stable: (channel == PackageReleaseChannel::Stable).then(|| ConsumerStableReleaseSpec {
            version_package: "app".to_owned(),
            release_pr: true,
            manifest_path: "Cargo.toml".to_owned(),
            release_plz_argv: vec![
                "mise".to_owned(),
                "--no-config".to_owned(),
                "--no-env".to_owned(),
                "--no-hooks".to_owned(),
                "exec".to_owned(),
                "rust@1.99.1".to_owned(),
                "release-plz@0.3.173".to_owned(),
                "--".to_owned(),
                "release-plz".to_owned(),
            ],
        }),
        preview: (channel == PackageReleaseChannel::Preview).then_some(
            ConsumerRollingPreviewSpec {
                release_id: 123,
                bootstrap_snapshot_sha256: None,
            },
        ),
    }
}

fn policy(channel: PackageReleaseChannel) -> PackageReleasePolicy {
    let plan = crate::package_release_publish::preview_artifacts::fixture::request().plan;
    PackageReleasePolicy {
        schema: PACKAGE_RELEASE_POLICY_SCHEMA_V1.to_owned(),
        channel,
        repository: plan.repository,
        default_branch: plan.default_branch,
        products: plan.products,
        companions: plan.companions,
        version: CargoVersionPolicy {
            version_package: "app".to_owned(),
            bindings: vec![CargoVersionBinding {
                package: "app".to_owned(),
                manifest_path: "app/Cargo.toml".to_owned(),
                workspace_manifest_path: "Cargo.toml".to_owned(),
            }],
        },
    }
}

fn tap_policy() -> velnor_actions_contract::config::HomebrewTapConfig {
    toml::from_str(r#"
enabled = true
repository = "example/homebrew-tap"
environment = "tap-update"
app_id_variable = "TAP_APP_ID"
app_private_key_secret = "TAP_APP_PRIVATE_KEY"
protection_observer = { app_id_variable = "TAP_PROTECTION_APP_ID", app_private_key_secret = "TAP_PROTECTION_APP_PRIVATE_KEY" }
merge = "squash-after-gates"
[destinations.preview]
formula = "Formula/app-preview.rb"
[destinations.stable]
formula = "Formula/app.rb"
[updater]
task = "package:update"
source = { mise_config = "mise.toml", working_directory = "." }
runner = { label = "ubuntu-26.04", platform = "linux_x64", executor = "hosted" }
inputs = ["mise.toml", "Formula/app-preview.rb", "Formula/app.rb"]
tools = ["python"]
timeout_minutes = 5
[review_completion]
check_name = "Review completion"
app_id_variable = "COMPLETION_APP_ID"
expected_reviewers = [{ app_id_variable = "REVIEWER_APP_ID" }]
"#).expect("typed native Tap policy")
}

fn job_names(text: &str) -> BTreeMap<String, String> {
    let mut jobs = BTreeMap::new();
    let mut id = None;
    for line in text.lines() {
        if let Some(key) = line
            .strip_prefix("  ")
            .filter(|s| !s.starts_with(' ') && s.ends_with(':'))
        {
            id = Some(key.trim_end_matches(':').to_owned());
        }
        if let Some(name) = line.strip_prefix("    name: ") {
            jobs.insert(
                id.take().expect("generated literal job owner"),
                name.trim_matches('"').to_owned(),
            );
        }
    }
    jobs
}

#[test]
fn actual_rendered_channel_and_tap_jobs_match_shared_runtime_role_mapping() {
    for channel in [
        PackageReleaseChannel::Preview,
        PackageReleaseChannel::Stable,
    ] {
        let spec = fixture(channel);
        let plan = spec
            .policy
            .resolve("0.6.4".to_owned())
            .expect("resolved fixture");
        let release = match channel {
            PackageReleaseChannel::Preview => {
                render_consumer_preview(&spec, &WorkflowRoutingPolicy::hosted_only())
            }
            PackageReleaseChannel::Stable => {
                render_consumer_binary_release(&spec, &WorkflowRoutingPolicy::hosted_only())
            }
        }
        .expect("actual native release rendering");
        let mut mapped = 0;
        for (id, name) in job_names(&release.bytes) {
            if id == "preview-eligibility" || id == "stable-prepare" {
                continue;
            }
            assert_eq!(
                release_job_name(&plan, &id).expect("emitted role has native authority mapping"),
                name,
                "{id}"
            );
            mapped += 1;
        }
        assert!(mapped >= 8);
        let tap = render_release_tap_reconcile(&spec, &WorkflowRoutingPolicy::hosted_only())
            .expect("actual native continuation rendering");
        let names = job_names(&tap.bytes);
        assert_eq!(names.len(), 4);
        for (id, name) in names {
            assert_eq!(
                continuation_job_name(channel, &id).expect("mapped continuation role"),
                name
            );
        }
    }
}
