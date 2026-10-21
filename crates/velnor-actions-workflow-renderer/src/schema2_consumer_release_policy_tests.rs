//! Public generation policy cannot infer mutable publication authority from URLs.
use super::*;

fn hosted_policy() -> crate::workflow_routing::WorkflowRoutingPolicy {
    crate::workflow_routing::WorkflowRoutingPolicy::hosted_only()
}
use velnor_actions_contract::package_release::PackageReleaseChannel;
#[test]
fn preview_requires_explicit_existing_release_authority() {
    let mut spec = fixture();
    spec.preview = None;
    assert!(render_consumer_preview(&spec, &hosted_policy()).is_err());
    let mut spec = fixture();
    spec.preview.as_mut().expect("preview").release_id = 0;
    assert!(render_consumer_preview(&spec, &hosted_policy()).is_err());
    let mut spec = fixture();
    spec.preview
        .as_mut()
        .expect("preview")
        .bootstrap_snapshot_sha256 = Some("A".repeat(64));
    assert!(render_consumer_preview(&spec, &hosted_policy()).is_err());
    let mut spec = fixture();
    spec.preview
        .as_mut()
        .expect("preview")
        .bootstrap_snapshot_sha256 = Some("b".repeat(64));
    let step = steps::operation(
        &spec,
        "publish-package-release-v1",
        true,
        TapInvocationContext::Preview,
    )
    .expect("fixed rolling policy");
    let Yaml::Map(fields) = step else {
        panic!("operation map");
    };
    let Some((_, Yaml::Map(env))) = fields.iter().find(|(key, _)| key == "env") else {
        panic!("operation environment");
    };
    assert!(env.contains(&("VELNOR_PREVIEW_RELEASE_ID".to_owned(), Yaml::str("123"))));
    assert!(env.contains(&(
        "VELNOR_PREVIEW_BOOTSTRAP_SNAPSHOT_SHA256".to_owned(),
        Yaml::str("b".repeat(64))
    )));
    let yaml = render_consumer_preview(&spec, &hosted_policy())
        .expect("fixed rolling workflow")
        .bytes;
    assert!(
        !job(&yaml, "preview-publisher", Some("preview-recovery")).contains("actions/checkout")
    );
}

#[test]
fn preview_rejects_unbounded_producer_limits_before_rendering() {
    for minutes in [0, 31, 360, 361, u16::MAX] {
        let mut spec = fixture();
        spec.producer_timeout_minutes = minutes;
        assert!(
            render_consumer_preview(&spec, &hosted_policy()).is_err(),
            "{minutes}"
        );
    }
}

#[test]
fn twenty_minute_producer_jobs_leave_control_signer_publication_and_updater_at_five() {
    let mut spec = with_tap();
    spec.producer_timeout_minutes = 20;
    let pairs = spec.policy.payload_pairs().expect("full plan").len();
    let rendered =
        render_consumer_preview(&spec, &hosted_policy()).expect("explicit twenty-minute producer");
    assert_eq!(
        rendered.bytes.matches("timeout-minutes: 25").count(),
        pairs + 1,
        "Only the payload jobs and assembly job must use the producer time limit plus the lifecycle allowance."
    );
    assert!(rendered.bytes.contains("timeout-minutes: 5"));
    assert!(!rendered.bytes.contains("timeout-minutes: 20"));
    for minutes in [1, 5, 6, 10, 15, 20] {
        spec.producer_timeout_minutes = minutes;
        assert!(
            render_consumer_preview(&spec, &hosted_policy()).is_ok(),
            "{minutes}"
        );
    }
    for minutes in [0, 31, 360, 361, u16::MAX] {
        spec.producer_timeout_minutes = minutes;
        assert!(
            render_consumer_preview(&spec, &hosted_policy()).is_err(),
            "{minutes}"
        );
    }
    let mut spec = with_tap();
    spec.homebrew_tap
        .as_mut()
        .expect("tap")
        .policy
        .updater
        .timeout_minutes = 6;
    assert!(
        render_consumer_preview(&spec, &hosted_policy()).is_err(),
        "producer authorization does not widen updater"
    );
}

#[test]
fn both_channels_bound_native_work_inside_larger_producer_jobs() {
    for channel in [
        PackageReleaseChannel::Preview,
        PackageReleaseChannel::Stable,
    ] {
        for minutes in [1, 5, 30] {
            let mut spec = channel_with_tap(channel);
            spec.producer_timeout_minutes = minutes;
            let prefix = if channel == PackageReleaseChannel::Preview {
                "preview"
            } else {
                "stable"
            };
            let mut producers: std::collections::BTreeSet<_> = spec
                .policy
                .payload_pairs()
                .expect("pairs")
                .iter()
                .map(|pair| spec.policy.payload_job_id(pair).expect("job ID"))
                .collect();
            producers.insert(format!("{prefix}-assemble"));
            let rendered = if channel == PackageReleaseChannel::Preview {
                render_consumer_preview(&spec, &hosted_policy())
            } else {
                crate::schema2::render_consumer_binary_release(&spec, &hosted_policy())
            }
            .expect("bounded native production");
            assert_job_envelopes(&rendered.bytes, &producers, minutes);
        }
    }
}

fn channel_with_tap(channel: PackageReleaseChannel) -> ConsumerReleaseSpec {
    if channel == PackageReleaseChannel::Preview {
        return with_tap();
    }
    let mut stable = super::stable::stable();
    let mut tap = with_tap().homebrew_tap.expect("tap");
    tap.policy.destinations.stable =
        Some(velnor_actions_contract::config::HomebrewTapDestination {
            formula: "Formula/scanner.rb".to_owned(),
        });
    tap.policy
        .updater
        .inputs
        .push("Formula/scanner.rb".to_owned());
    stable.homebrew_tap = Some(tap);
    stable
}

fn assert_job_envelopes(yaml: &str, producers: &std::collections::BTreeSet<String>, minutes: u16) {
    let mut observed = std::collections::BTreeSet::new();
    let mut job_id = None;
    let mut jobs = 0;
    let mut timeouts = 0;
    for line in yaml.split_once("jobs:\n").expect("jobs map").1.lines() {
        if line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':') {
            job_id = Some(line.trim().trim_end_matches(':'));
            jobs += 1;
        }
        if let Some(timeout) = line.strip_prefix("    timeout-minutes: ") {
            let id = job_id.expect("timeout belongs to a job");
            let expected = if producers.contains(id) {
                observed.insert(id.to_owned());
                minutes + 5
            } else {
                5
            };
            assert_eq!(
                timeout.parse::<u16>().expect("literal timeout"),
                expected,
                "{id}/{minutes}"
            );
            timeouts += 1;
        }
    }
    assert_eq!(jobs, timeouts, "every concrete job has one literal timeout");
    assert_eq!(
        &observed, producers,
        "all payloads and assembly get lifecycle headroom"
    );
}
