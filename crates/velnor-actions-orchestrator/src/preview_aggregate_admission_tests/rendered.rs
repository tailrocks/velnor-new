use super::*;

use std::collections::BTreeMap;
use velnor_actions_contract::VerificationRunner;
use velnor_actions_contract::targets::ReleaseTarget;
use velnor_actions_workflow_renderer::{
    HelperProvenance, MiseSetup, provision_acquire_step,
    schema2::{
        ConsumerReleaseSpec, ConsumerStableReleaseSpec, render_consumer_binary_release,
        render_consumer_preview,
    },
};

impl Fixture {
    pub(super) fn assert_rendered_sequence(&self) {
        let channel = self.proof.request.plan.channel;
        let spec = fixture(channel);
        let rendered = match channel {
            PackageReleaseChannel::Preview => render_consumer_preview(&spec),
            PackageReleaseChannel::Stable => render_consumer_binary_release(&spec),
        }
        .expect("actual native renderer");
        let start = rendered
            .bytes
            .find(&format!("  {}:\n", channel.signer_job_id()))
            .expect("signer");
        let end = rendered.bytes[start..]
            .find(&format!(
                "\n  {}:",
                channel.signer_job_id().replace("-signer", "-publisher")
            ))
            .expect("next job");
        let signer = &rendered.bytes[start..start + end];
        let admission = signer
            .find("VELNOR_INTERNAL_OP: admit-release-aggregate-v1")
            .expect("aggregate admission");
        let sign = signer
            .find("VELNOR_INTERNAL_OP: sign-package-release-v1")
            .expect("real signer");
        assert!(
            admission < sign,
            "real workflow preserves admission before signing"
        );
        assert!(signer.contains(&format!(
            "needs.{}.outputs.artifact_id",
            channel.assemble_job_id()
        )));
        assert!(signer.contains("/velnor/release-signed"));
        assert!(
            rendered.bytes.contains("/velnor/release-output"),
            "assembler upload path"
        );
    }
}

fn policy(channel: PackageReleaseChannel) -> velnor_actions_contract::PackageReleasePolicy {
    let fixture = Fixture::new(channel);
    let plan = fixture.proof.request.plan;
    velnor_actions_contract::PackageReleasePolicy {
        schema: velnor_actions_contract::PACKAGE_RELEASE_POLICY_SCHEMA_V1.into(),
        channel,
        repository: plan.repository,
        default_branch: plan.default_branch,
        products: plan.products,
        companions: plan.companions,
        version: velnor_actions_contract::CargoVersionPolicy {
            version_package: "cli".into(),
            bindings: vec![velnor_actions_contract::CargoVersionBinding {
                package: "cli".into(),
                manifest_path: "Cargo.toml".into(),
                workspace_manifest_path: "Cargo.toml".into(),
            }],
        },
    }
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
        policy: policy(channel),
        producer_runner: VerificationRunner::Macos26Arm64,
        producer_timeout_minutes: 20,
        publication_timeout_minutes: velnor_actions_contract::JobTimeout::DEFAULT,
        linux_acquire: acquire.clone(),
        producer_acquire: acquire,
        producer_acquire_by_target: producer_acquires(channel, "0.1.33"),
        producer_setup: setup.clone(),
        producer_setup_by_target: producer_setups(channel, &setup),
        linux_setup: setup,
        install_gh_argv: vec!["mise".into(), "install".into(), "gh@2.102.1".into()],
        verify_rust_argv: vec![
            "sh".to_owned(),
            "-c".to_owned(),
            velnor_actions_workflow_renderer::toolchain_env::with_credential_unset_script(
                "echo verify-rust",
            ),
        ],
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
            version_package: "cli".into(),
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
    }
}

fn producer_acquires(
    channel: PackageReleaseChannel,
    version: &str,
) -> BTreeMap<ReleaseTarget, velnor_actions_contract::Step> {
    if channel != PackageReleaseChannel::Preview {
        return BTreeMap::new();
    }
    ReleaseTarget::ALL
        .into_iter()
        .map(|target| {
            let asset = format!("velnor-actions-{version}-{}", target.triple());
            let checksum = match target {
                ReleaseTarget::LinuxX86_64 => "sha256sum -c -",
                ReleaseTarget::MacosArm64 | ReleaseTarget::MacosX86_64 => "shasum -a 256 -c -",
            };
            let step = provision_acquire_step(
                &HelperProvenance::ReleaseAsset {
                    url: format!(
                        "https://github.com/tailrocks/velnor-new/releases/download/v{version}/{asset}"
                    ),
                    sha256: "c".repeat(64),
                    commit: "b".repeat(40),
                },
                vec![
                    "sh".into(),
                    "-c".into(),
                    format!(
                        r#"curl -fsSL "$VELNOR_ASSET_URL" -o "$RUNNER_TEMP/velnor/bin/velnor-actions-{version}" && printf '%s  %s\n' "$VELNOR_ASSET_SHA256" "$RUNNER_TEMP/velnor/bin/velnor-actions-{version}" | {checksum}"#
                    ),
                ],
            )
            .expect("target-specific typed acquisition");
            (target, step)
        })
        .collect()
}

fn producer_setups(
    channel: PackageReleaseChannel,
    setup: &MiseSetup,
) -> BTreeMap<ReleaseTarget, MiseSetup> {
    if channel == PackageReleaseChannel::Preview {
        ReleaseTarget::ALL
            .into_iter()
            .map(|target| (target, setup.clone()))
            .collect()
    } else {
        BTreeMap::new()
    }
}
