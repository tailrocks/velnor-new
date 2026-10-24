use super::super::render_consumer_preview;
use super::{hosted_policy, job, stable, with_tap};
use crate::schema2::{render_consumer_binary_release, render_release_tap_reconcile};
use velnor_actions_contract::JobTimeout;

fn timeout_line<'a>(yaml: &'a str, name: &str, next: Option<&str>) -> &'a str {
    job(yaml, name, next)
        .lines()
        .find(|line| line.trim_start().starts_with("timeout-minutes:"))
        .expect("job timeout")
}

#[test]
fn publication_timeout_only_extends_preview_admission_and_shared_publish_recovery_jobs() {
    let preview_default = with_tap();
    let preview_default_yaml = render_consumer_preview(&preview_default, &hosted_policy())
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
    let preview_yaml = render_consumer_preview(&preview, &hosted_policy())
        .expect("preview")
        .bytes;
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
        render_release_tap_reconcile(&preview_default, &hosted_policy())
            .expect("default reconciliation")
            .bytes,
        render_release_tap_reconcile(&preview, &hosted_policy())
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
    let stable_default_yaml = render_consumer_binary_release(&stable_default, &hosted_policy())
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
    let stable_yaml = render_consumer_binary_release(&stable, &hosted_policy())
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
