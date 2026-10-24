//! Stable uses the same product matrix and isolated custody as per-source preview.
use super::{fixture, job, with_tap};
use crate::schema2::{
    ConsumerReleaseSpec, ConsumerStableReleaseSpec, render_consumer_binary_release,
    render_consumer_preview, render_release_tap_reconcile,
};
use velnor_actions_contract::{JobTimeout, package_release::PackageReleaseChannel};
pub(super) fn stable() -> ConsumerReleaseSpec {
    let mut spec = fixture();
    spec.policy.channel = PackageReleaseChannel::Stable;
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
            "release-plz@0.4.0".to_owned(),
            "--".to_owned(),
            "release-plz".to_owned(),
        ],
    });
    spec
}

fn timeout_line<'a>(yaml: &'a str, name: &str, next: Option<&str>) -> &'a str {
    job(yaml, name, next)
        .lines()
        .find(|line| line.trim_start().starts_with("timeout-minutes:"))
        .expect("job timeout")
}

#[test]
fn publication_timeout_only_extends_preview_admission_and_shared_publish_recovery_jobs() {
    let preview_default = with_tap();
    let preview_default_yaml = render_consumer_preview(&preview_default)
        .expect("default preview")
        .bytes;
    assert_eq!(preview_default.publication_timeout_minutes.minutes(), 5);
    for (name, next) in [
        ("preview-eligibility", Some("preview-payload")),
        ("preview-signer", Some("preview-publisher")),
    ] {
        assert!(timeout_line(&preview_default_yaml, name, next).contains("timeout-minutes: 5"));
    }
    assert!(
        timeout_line(&preview_default_yaml, "preview-publisher", None)
            .contains("timeout-minutes: 32")
    );
    assert!(
        timeout_line(&preview_default_yaml, "preview-recovery", None)
            .contains("timeout-minutes: 22")
    );

    let mut preview = preview_default.clone();
    preview.publication_timeout_minutes = JobTimeout::new(15).expect("bounded timeout");
    let preview_yaml = render_consumer_preview(&preview).expect("preview").bytes;
    assert!(
        timeout_line(
            &preview_yaml,
            "preview-eligibility",
            Some("preview-payload")
        )
        .contains("timeout-minutes: 15")
    );
    assert!(timeout_line(&preview_yaml, "preview-publisher", None).contains("timeout-minutes: 42"));
    assert!(timeout_line(&preview_yaml, "preview-recovery", None).contains("timeout-minutes: 32"));
    for (name, next) in [
        ("preview-signer", Some("preview-publisher")),
        ("preview-tap-prepare", Some("preview-tap-writer")),
        ("preview-tap-writer", Some("preview-tap-merge")),
        ("preview-tap-merge", None),
    ] {
        assert!(timeout_line(&preview_yaml, name, next).contains("timeout-minutes: 5"));
    }
    for (name, next) in [
        ("preview-payload", Some("preview-assemble")),
        ("preview-assemble", Some("preview-signer")),
    ] {
        assert_eq!(
            timeout_line(&preview_default_yaml, name, next),
            timeout_line(&preview_yaml, name, next),
            "{name} native timeout remains source-configured"
        );
    }
    assert_eq!(
        render_release_tap_reconcile(&preview_default)
            .expect("default reconciliation")
            .bytes,
        render_release_tap_reconcile(&preview)
            .expect("reconciliation")
            .bytes,
        "separate reconciliation workflow keeps its timeout graph"
    );
}

#[test]
fn publication_timeout_only_extends_stable_shared_publish_recovery_jobs() {
    let mut stable_default = stable();
    let mut tap = with_tap().homebrew_tap.expect("tap policy");
    tap.policy.destinations.stable =
        Some(velnor_actions_contract::config::HomebrewTapDestination {
            formula: "Formula/scanner.rb".to_owned(),
        });
    tap.policy
        .updater
        .inputs
        .push("Formula/scanner.rb".to_owned());
    stable_default.homebrew_tap = Some(tap.clone());
    let stable_default_yaml = render_consumer_binary_release(&stable_default)
        .expect("default stable")
        .bytes;
    for (name, next) in [
        ("stable-eligibility", Some("stable-prepare")),
        ("stable-signer", Some("stable-publisher")),
    ] {
        assert!(timeout_line(&stable_default_yaml, name, next).contains("timeout-minutes: 5"));
    }
    assert!(
        timeout_line(
            &stable_default_yaml,
            "stable-publisher",
            Some("stable-recovery")
        )
        .contains("timeout-minutes: 32")
    );
    assert!(
        timeout_line(&stable_default_yaml, "stable-recovery", None).contains("timeout-minutes: 22")
    );
    let mut stable = stable_default;
    stable.publication_timeout_minutes = JobTimeout::new(15).expect("bounded timeout");
    let stable_yaml = render_consumer_binary_release(&stable)
        .expect("stable")
        .bytes;
    assert!(
        timeout_line(&stable_yaml, "stable-publisher", Some("stable-recovery"))
            .contains("timeout-minutes: 42")
    );
    assert!(timeout_line(&stable_yaml, "stable-recovery", None).contains("timeout-minutes: 32"));
    for (name, next) in [
        ("stable-eligibility", Some("stable-prepare")),
        ("stable-signer", Some("stable-publisher")),
        ("stable-payload-000", Some("stable-payload-001")),
        ("stable-tap-prepare", Some("stable-tap-writer")),
        ("stable-tap-writer", Some("stable-tap-merge")),
        ("stable-tap-merge", None),
    ] {
        assert_eq!(
            timeout_line(&stable_default_yaml, name, next),
            timeout_line(&stable_yaml, name, next),
            "{name} timeout stays unchanged"
        );
    }
    assert!(
        timeout_line(&stable_yaml, "stable-eligibility", Some("stable-prepare"))
            .contains("timeout-minutes: 5")
    );
    assert!(
        timeout_line(&stable_yaml, "stable-signer", Some("stable-publisher"))
            .contains("timeout-minutes: 5")
    );
}

#[test]
fn stable_manual_graph_retains_complete_inventory_and_separates_custody() {
    let rendered = render_consumer_binary_release(&stable()).expect("shared stable graph");
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
    let yaml = render_consumer_binary_release(&stable())
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
    let yaml = render_consumer_binary_release(&spec)
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
    assert!(render_consumer_binary_release(&spec).is_err());
    let mut spec = stable();
    spec.policy.channel = PackageReleaseChannel::Preview;
    assert!(render_consumer_binary_release(&spec).is_err());
    let mut spec = stable();
    spec.stable.as_mut().expect("stable").version_package =
        "x\"\npre_release_hook = ['evil']".to_owned();
    assert!(render_consumer_binary_release(&spec).is_err());
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
    let yaml = render_consumer_binary_release(&spec)
        .expect("stable tap")
        .bytes;
    assert_eq!(yaml.matches("timeout-minutes: 25").count(), 7);
    let prepare = job(&yaml, "stable-tap-prepare", Some("stable-tap-writer"));
    assert!(!prepare.contains("create-github-app-token"));
    assert!(prepare.contains("admit-homebrew-tap-v1"));
    assert!(prepare.find("admit-homebrew-tap-v1") < prepare.find("actions/checkout"));
    assert!(prepare.contains("needs.stable-eligibility.outputs.source_sha"));
    let merge = job(&yaml, "stable-tap-merge", None);
    assert!(merge.contains("merge-homebrew-tap-pr-v1"));
    assert!(!merge.contains("actions/checkout"));
    assert!(!merge.contains("timeout-minutes: 25"));
    spec.producer_timeout_minutes = 31;
    assert!(render_consumer_binary_release(&spec).is_err());
    let mut spec = stable();
    spec.stable.as_mut().expect("stable").release_plz_argv[0] = "bash".to_owned();
    assert!(render_consumer_binary_release(&spec).is_err());
}

#[test]
fn stable_tap_reconciliation_is_valid_without_dispatch_inputs() {
    use velnor_actions_contract::config::HomebrewTapDestination;

    let mut spec = stable();
    let mut tap = super::with_tap().homebrew_tap.expect("tap policy");
    tap.policy.destinations.stable = Some(HomebrewTapDestination {
        formula: "Formula/scanner.rb".to_owned(),
    });
    tap.policy
        .updater
        .inputs
        .push("Formula/scanner.rb".to_owned());
    spec.homebrew_tap = Some(tap);

    let rendered = render_release_tap_reconcile(&spec).expect("stable reconciliation workflow");
    assert_eq!(rendered.path, ".github/workflows/stable-tap-reconcile.yml");
    assert!(
        rendered.bytes.contains("name: Stable tap reconciliation"),
        "{}",
        rendered.bytes
    );
    assert!(!rendered.bytes.contains("inputs."), "{}", rendered.bytes);
    assert!(!rendered.bytes.contains("env: {}"), "{}", rendered.bytes);
    assert!(
        rendered
            .bytes
            .contains("VELNOR_STABLE_VERSION_PACKAGE: scanner")
    );
    assert!(!rendered.bytes.contains("VELNOR_STABLE_OPERATION:"));
    for operation in [
        "admit-homebrew-tap-v1",
        "prepare-homebrew-tap-v1",
        "write-homebrew-tap-pr-v1",
        "merge-homebrew-tap-pr-v1",
    ] {
        assert!(rendered.bytes.contains(operation), "missing {operation}");
    }

    let stable_release = render_consumer_binary_release(&spec)
        .expect("stable publisher")
        .bytes;
    assert!(stable_release.contains("VELNOR_STABLE_OPERATION: ${{ inputs.operation }}"));
}
