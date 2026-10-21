//! Feed the actual generated request path into the real local admission loader.
use super::*;
use velnor_actions_contract::VerificationRunner;
use velnor_actions_workflow_renderer::{
    HelperProvenance, MiseSetup, provision_acquire_step,
    schema2::{
        ConsumerReleaseSpec, ConsumerRollingPreviewSpec, ConsumerStableReleaseSpec,
        render_consumer_binary_release, render_consumer_preview,
    },
};

pub(super) fn request_path(channel: PackageReleaseChannel, operation: &str) -> String {
    let spec = fixture(channel);
    let rendered = match channel {
        PackageReleaseChannel::Preview => render_consumer_preview(&spec),
        PackageReleaseChannel::Stable => render_consumer_binary_release(&spec),
    }
    .expect("actual native workflow");
    let marker = format!("VELNOR_INTERNAL_OP: {operation}");
    let lines: Vec<_> = rendered.bytes.lines().collect();
    let positions: Vec<_> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim().trim_matches('"') == marker)
        .map(|(i, _)| i)
        .collect();
    assert_eq!(positions.len(), 1, "unique actual operation env");
    let line = lines[positions[0] + 1].trim();
    let value = line
        .strip_prefix("VELNOR_REQUEST_FILE: ")
        .expect("same operation request");
    value.trim_matches('"').to_owned()
}

fn fixture(channel: PackageReleaseChannel) -> ConsumerReleaseSpec {
    let acquire = provision_acquire_step(&HelperProvenance::ReleaseAsset {
        url: "https://github.com/tailrocks/velnor-new/releases/download/v0.1.33/velnor-actions-0.1.33-x86_64-unknown-linux-gnu".into(),
        sha256: "a".repeat(64), commit: "b".repeat(40),
    }, vec!["sh".into(), "-c".into(), r#"curl -fsSL "$VELNOR_ASSET_URL" -o "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.33" && printf '%s  %s\n' "$VELNOR_ASSET_SHA256" "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.33" | sha256sum -c -"#.into()]).expect("typed acquisition");
    let setup = MiseSetup {
        uses: "jdx/mise-action@fc42bcc7ddbf51124c30f95a877a0c5168a3bfde".into(),
        version: "2026.10.17".into(),
        sha256: "a".repeat(64),
    };
    ConsumerReleaseSpec {
        generator_version: "0.1.33".into(),
        checkout_lfs: false,
        policy: tests::policy(channel),
        producer_runner: VerificationRunner::Macos26Arm64,
        producer_timeout_minutes: 20,
        linux_acquire: acquire.clone(),
        producer_acquire: acquire,
        producer_setup: setup.clone(),
        linux_setup: setup,
        install_gh_argv: vec!["mise".into(), "install".into(), "gh@2.102.1".into()],
        install_rust_argv: vec!["mise".into(), "install".into(), "rust@1.99.1".into()],
        producer_source_prep: vec![],
        producer_cargo_env: [
            "CARGO_HOME",
            "MISE_CARGO_HOME",
            "RUSTUP_HOME",
            "MISE_RUSTUP_HOME",
        ]
        .into_iter()
        .map(|k| (k.into(), "${{ runner.temp }}/velnor/cargo".into()))
        .collect(),
        gh_argv: vec!["gh".into()],
        homebrew_tap: None,
        stable: (channel == PackageReleaseChannel::Stable).then(|| ConsumerStableReleaseSpec {
            version_package: "jackin".into(),
            release_pr: true,
            manifest_path: "Cargo.toml".into(),
            release_plz_argv: vec![
                "mise".into(),
                "--no-config".into(),
                "--no-env".into(),
                "--no-hooks".into(),
                "exec".into(),
                "rust@1.99.1".into(),
                "release-plz@0.3.173".into(),
                "--".into(),
                "release-plz".into(),
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
