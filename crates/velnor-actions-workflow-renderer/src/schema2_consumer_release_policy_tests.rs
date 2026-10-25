//! Public generation policy fixes preview and stable publication authority.
use super::*;

fn hosted_policy() -> crate::workflow_routing::WorkflowRoutingPolicy {
    crate::workflow_routing::WorkflowRoutingPolicy::hosted_only()
}
use velnor_actions_contract::package_release::PackageReleaseChannel;
#[test]
fn preview_has_no_mutable_release_or_bootstrap_environment() {
    let spec = fixture();
    let step = steps::operation(
        &spec,
        "publish-package-release-v1",
        true,
        TapInvocationContext::Preview,
    )
    .expect("immutable preview policy");
    let Yaml::Map(fields) = step else {
        panic!("operation map");
    };
    let Some((_, Yaml::Map(env))) = fields.iter().find(|(key, _)| key == "env") else {
        panic!("operation environment");
    };
    assert!(
        !env.iter()
            .any(|(key, _)| key == "VELNOR_PREVIEW_RELEASE_ID")
    );
    assert!(
        !env.iter()
            .any(|(key, _)| key == "VELNOR_PREVIEW_BOOTSTRAP_SNAPSHOT_SHA256")
    );
    let yaml = render_consumer_preview(&spec, &hosted_policy())
        .expect("immutable preview workflow")
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
    let rendered =
        render_consumer_preview(&spec, &hosted_policy()).expect("explicit twenty-minute producer");
    assert_eq!(
        rendered
            .bytes
            .matches("timeout-minutes: ${{ matrix.timeout_minutes }}")
            .count(),
        1,
        "The preview payload matrix gets its timeout from authenticated source output."
    );
    assert_eq!(
        rendered
            .bytes
            .matches("timeout-minutes: ${{ fromJSON(needs.preview-eligibility.outputs.producer_timeout_minutes) }}")
            .count(),
        1,
        "Assembly gets its timeout from authenticated source output."
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
    let spec = with_tap();
    let rendered =
        render_consumer_preview(&spec, &hosted_policy()).expect("native tap preparation");
    let prepare = rendered
        .bytes
        .split("  preview-tap-prepare:")
        .nth(1)
        .expect("preparation job")
        .split("  preview-tap-writer:")
        .next()
        .expect("preparation body");
    assert!(prepare.contains("timeout-minutes: 15"));
    let mut spec = with_custom_updater();
    spec.homebrew_tap
        .as_mut()
        .expect("tap")
        .policy
        .updater
        .as_mut()
        .expect("configured custom updater")
        .timeout_minutes = 6;
    assert!(
        render_consumer_preview(&spec, &hosted_policy()).is_err(),
        "producer authorization does not widen updater"
    );
}

fn with_custom_updater() -> ConsumerReleaseSpec {
    use velnor_actions_contract::config::{
        CheckExecutor, CheckPlatform, CheckRunner, MiseTaskSource, TapUpdaterBinding,
    };
    let mut spec = with_tap();
    let tap = spec.homebrew_tap.as_mut().expect("tap");
    tap.formula_spec = None;
    tap.policy.updater = Some(TapUpdaterBinding {
        task: "tap:update".to_owned(),
        source: MiseTaskSource {
            mise_config: "mise.toml".to_owned(),
            working_directory: ".".to_owned(),
        },
        runner: CheckRunner {
            label: "ubuntu-24.04".to_owned(),
            platform: CheckPlatform::LinuxX64,
            executor: CheckExecutor::Hosted,
            container: None,
        },
        inputs: vec![
            "mise.toml".to_owned(),
            "Formula/scanner-preview.rb".to_owned(),
        ],
        tools: vec!["ruby".to_owned()],
        timeout_minutes: 5,
    });
    spec
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
            let mut producers: std::collections::BTreeSet<_> =
                if channel == PackageReleaseChannel::Preview {
                    ["preview-payload".to_owned()].into_iter().collect()
                } else {
                    spec.policy
                        .payload_pairs()
                        .expect("pairs")
                        .iter()
                        .map(|pair| spec.policy.payload_job_id(pair).expect("job ID"))
                        .collect()
                };
            producers.insert(format!("{prefix}-assemble"));
            let rendered = if channel == PackageReleaseChannel::Preview {
                render_consumer_preview(&spec, &hosted_policy())
            } else {
                crate::schema2::render_consumer_binary_release(&spec, &hosted_policy())
            }
            .expect("bounded native production");
            if channel == PackageReleaseChannel::Preview {
                assert!(
                    rendered
                        .bytes
                        .contains("timeout-minutes: ${{ matrix.timeout_minutes }}")
                );
                assert!(rendered.bytes.contains("timeout-minutes: ${{ fromJSON(needs.preview-eligibility.outputs.producer_timeout_minutes) }}"));
            } else {
                assert_job_envelopes(&rendered.bytes, &producers, minutes);
            }
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
            let expected = if id.ends_with("-publisher") {
                32
            } else if id.ends_with("-recovery") {
                22
            } else if id.ends_with("-tap-prepare") {
                15
            } else if producers.contains(id) {
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
