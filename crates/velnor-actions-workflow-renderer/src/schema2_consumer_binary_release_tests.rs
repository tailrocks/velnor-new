//! Stable uses the same product matrix and isolated custody as rolling preview.
use super::{fixture, job};

fn hosted_policy() -> crate::workflow_routing::WorkflowRoutingPolicy {
    crate::workflow_routing::WorkflowRoutingPolicy::hosted_only()
}
use crate::schema2::{
    ConsumerReleaseSpec, ConsumerStableReleaseSpec, render_consumer_binary_release,
};
use velnor_actions_contract::package_release::PackageReleaseChannel;
pub(super) fn stable() -> ConsumerReleaseSpec {
    let mut spec = fixture();
    spec.policy.channel = PackageReleaseChannel::Stable;
    spec.preview = None;
    spec.stable = Some(ConsumerStableReleaseSpec {
        version_package: "scanner".to_owned(),
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
    });
    spec
}
#[test]
fn stable_manual_graph_retains_complete_inventory_and_separates_custody() {
    let rendered =
        render_consumer_binary_release(&stable(), &hosted_policy()).expect("shared stable graph");
    assert_eq!(rendered.path, ".github/workflows/binary-release.yml");
    let yaml = rendered.bytes;
    let trigger = &yaml[..yaml.find("jobs:").expect("jobs")];
    assert!(trigger.contains("workflow_dispatch:"));
    for forbidden in ["workflow_run:", "push:", "schedule:"] {
        assert!(!trigger.contains(forbidden));
    }
    assert!(trigger.contains("default: prepare"));
    for (index, pair) in [
        ("capsule", "aarch64-apple-darwin"),
        ("capsule", "aarch64-unknown-linux-gnu"),
        ("scanner", "aarch64-apple-darwin"),
        ("scanner", "aarch64-unknown-linux-gnu"),
        ("scanner", "x86_64-apple-darwin"),
        ("scanner", "x86_64-unknown-linux-gnu"),
    ]
    .iter()
    .enumerate()
    {
        let id = format!("stable-payload-{index:03}");
        let payload = job(&yaml, &id, None);
        assert!(payload.contains(&format!("Build {} / {}", pair.0, pair.1)));
    }
    let signer = job(&yaml, "stable-signer", Some("stable-publisher"));
    assert!(signer.contains("attestations: write"));
    assert!(signer.contains("id-token: write"));
    assert!(!signer.contains("actions/checkout"));
    assert!(!signer.contains("contents: write"));
    let publisher = job(&yaml, "stable-publisher", Some("stable-recovery"));
    assert!(publisher.contains("contents: write"));
    assert!(!publisher.contains("actions/checkout"));
    assert!(!publisher.contains("id-token: write"));
    assert!(!yaml.contains("raw-binary"));
    assert!(!yaml.contains("actions: write"));
}
#[test]
fn stable_intent_precedes_tag_mutation_and_observed_origin_precedes_builds() {
    let yaml = render_consumer_binary_release(&stable(), &hosted_policy())
        .expect("stable")
        .bytes;
    let tag = job(&yaml, "stable-tag", Some("stable-payload-000"));
    let intent = tag
        .find("write-stable-tag-intent-v1")
        .expect("intent writer");
    let retained = tag.find("id: intent-upload").expect("durable intent");
    let origin = tag
        .find("write-stable-tag-origin-v1")
        .expect("observed origin");
    let upload = tag.find("id: origin-upload").expect("origin artifact");
    assert!(intent < retained && retained < origin && origin < upload);
    assert!(!tag.contains("actions/checkout"));
    assert!(!tag.contains("id: release-plz"));
    assert!(!tag.contains("release-plz-output.json"));
    let prepare = job(&yaml, "stable-prepare", Some("stable-tag"));
    assert!(prepare.contains("git_release_enable = false"));
    assert!(prepare.contains("git_tag_enable = false"));
    assert!(prepare.contains("publish = false"));
    assert!(prepare.contains("core.hooksPath"));
    assert!(prepare.contains("tag.gpgSign"));
    assert!(prepare.contains("commit.gpgSign"));
    assert!(prepare.contains("persist-credentials: false"));
    assert!(tag.contains("VELNOR_STABLE_TAG_INTENT_ARTIFACT_DIGEST"));
    let leaf = job(&yaml, "stable-payload-000", Some("stable-payload-001"));
    assert!(leaf.find("admit-stable-tag-origin-v1") < leaf.find("actions/checkout"));
    assert!(leaf.contains("VELNOR_STABLE_TAG_ORIGIN_ADMISSION_DIGEST"));
    assert!(!leaf.contains("environment:"));
    assert!(!leaf.contains("contents: write"));
}
#[test]
fn stable_recovery_cannot_rebuild_and_preparation_is_explicitly_disabled() {
    let mut spec = stable();
    spec.stable.as_mut().expect("stable").release_pr = false;
    let yaml = render_consumer_binary_release(&spec, &hosted_policy())
        .expect("no prepare")
        .bytes;
    let trigger = &yaml[..yaml.find("jobs:").expect("jobs")];
    assert!(!trigger.contains("default:"));
    assert!(!trigger.contains("prepare"));
    assert!(!yaml.contains("  stable-prepare:"));
    let recovery = job(&yaml, "stable-recovery", None);
    assert!(recovery.contains("inputs.original_run_id != ''"));
    assert!(recovery.contains("inputs.original_run_attempt != ''"));
    assert!(recovery.contains("recover-package-release-v1"));
    assert!(recovery.contains("needs.stable-eligibility.outputs.ci_run_id"));
    assert!(!recovery.contains("needs.stable-tag"));
    assert!(!recovery.contains("actions/checkout"));
    assert!(!recovery.contains("execute-release-producer-v1"));
    let payload = job(&yaml, "stable-payload-000", Some("stable-payload-001"));
    assert!(payload.contains("inputs.original_run_id == ''"));
    assert!(payload.contains("inputs.original_run_attempt == ''"));
}
#[test]
fn stable_rejects_legacy_inventory_and_invalid_authority_before_rendering() {
    let spec = stable();
    let raw = String::from_utf8(spec.policy.canonical_bytes().expect("plan"))
        .expect("utf8")
        .replace("tar-gz", "raw-binary");
    assert!(velnor_actions_contract::parse_package_release_policy(raw.as_bytes()).is_err());
    let mut spec = stable();
    spec.stable = None;
    assert!(render_consumer_binary_release(&spec, &hosted_policy()).is_err());
    let mut spec = stable();
    spec.policy.channel = PackageReleaseChannel::Preview;
    assert!(render_consumer_binary_release(&spec, &hosted_policy()).is_err());
    let mut spec = stable();
    spec.stable.as_mut().expect("stable").version_package =
        "x\"\npre_release_hook = ['evil']".to_owned();
    assert!(render_consumer_binary_release(&spec, &hosted_policy()).is_err());
}

#[test]
fn stable_shares_bounded_production_and_channel_specific_tap_destinations() {
    use velnor_actions_contract::config::HomebrewTapDestination;
    let mut spec = stable();
    spec.producer_timeout_minutes = 20;
    let mut tap = super::with_tap().homebrew_tap.expect("tap");
    tap.policy.destinations.stable = Some(HomebrewTapDestination {
        formula: "Formula/scanner.rb".to_owned(),
    });
    tap.policy
        .updater
        .inputs
        .push("Formula/scanner.rb".to_owned());
    spec.homebrew_tap = Some(tap);
    let yaml = render_consumer_binary_release(&spec, &hosted_policy())
        .expect("stable tap")
        .bytes;
    assert_eq!(yaml.matches("timeout-minutes: 20").count(), 7);
    let prepare = job(&yaml, "stable-tap-prepare", Some("stable-tap-writer"));
    assert!(!prepare.contains("create-github-app-token"));
    assert!(prepare.contains("admit-homebrew-tap-v1"));
    assert!(prepare.find("admit-homebrew-tap-v1") < prepare.find("actions/checkout"));
    assert!(prepare.contains("needs.stable-eligibility.outputs.source_sha"));
    let merge = job(&yaml, "stable-tap-merge", None);
    assert!(merge.contains("merge-homebrew-tap-pr-v1"));
    assert!(!merge.contains("actions/checkout"));
    assert!(!merge.contains("timeout-minutes: 20"));
    spec.producer_timeout_minutes = 21;
    assert!(render_consumer_binary_release(&spec, &hosted_policy()).is_err());
    let mut spec = stable();
    spec.stable.as_mut().expect("stable").release_plz_argv[0] = "bash".to_owned();
    assert!(render_consumer_binary_release(&spec, &hosted_policy()).is_err());
}

#[test]
fn scale_set_rejects_the_native_macos_binary_build_job() {
    let policy = crate::workflow_routing::test_policy(
        velnor_actions_contract::ExecutionMode::ScaleSet,
        crate::schema2::Schema2WorkflowRequest::canonical_scale_set().expect("selector"),
    );
    let error = render_consumer_binary_release(&stable(), &policy)
        .expect_err("macOS binary build cannot run on a Linux Scale Set");
    assert!(
        error.to_string().contains(
            "unsupported_darwin_workload:.github/workflows/binary-release.yml:stable-payload-000:"
        ),
        "{error}"
    );
}
