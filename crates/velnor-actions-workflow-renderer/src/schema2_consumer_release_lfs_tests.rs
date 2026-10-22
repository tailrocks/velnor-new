//! Native source access honors the same opt-in as ordinary workflow jobs.
use super::*;

fn checkout_count(node: &Yaml, expected_lfs: bool) -> usize {
    match node {
        Yaml::Map(fields) => {
            let checkout = fields.iter().any(|(key, value)| {
                key == "uses"
                    && matches!(value, Yaml::Str(uses) if uses.starts_with("actions/checkout@"))
            });
            if checkout {
                let Some((_, Yaml::Map(inputs))) = fields.iter().find(|(key, _)| key == "with")
                else {
                    panic!("checkout inputs");
                };
                let lfs = inputs.iter().find(|(key, _)| key == "lfs");
                assert_eq!(
                    lfs.map(|(_, value)| value),
                    expected_lfs.then(|| Yaml::str("true")).as_ref()
                );
                assert!(inputs.contains(&("fetch-depth".to_owned(), Yaml::Int(0))));
                assert!(inputs.contains(&("persist-credentials".to_owned(), Yaml::Bool(false))));
                assert!(inputs.iter().any(|(key, _)| key == "ref"));
            }
            usize::from(checkout)
                + fields
                    .iter()
                    .map(|(_, value)| checkout_count(value, expected_lfs))
                    .sum::<usize>()
        }
        Yaml::Seq(values) => values
            .iter()
            .map(|value| checkout_count(value, expected_lfs))
            .sum(),
        _ => 0,
    }
}

#[test]
fn native_checkout_lfs_opt_in_preserves_all_other_inputs_and_default_bytes() {
    let mut spec = fixture();
    let original = steps::history_checkout(&spec);
    assert_eq!(checkout_count(&original, false), 1);
    spec.checkout_lfs = true;
    let mut enabled = steps::history_checkout(&spec);
    assert_eq!(checkout_count(&enabled, true), 1);
    let Yaml::Map(fields) = &mut enabled else {
        panic!("checkout");
    };
    let Some((_, Yaml::Map(inputs))) = fields.iter_mut().find(|(key, _)| key == "with") else {
        panic!("inputs");
    };
    inputs.retain(|(key, _)| key != "lfs");
    assert_eq!(
        crate::yaml::render_yaml(&enabled),
        crate::yaml::render_yaml(&original)
    );
}

#[test]
fn native_production_and_both_tap_channels_apply_lfs_without_privileged_checkouts() {
    for enabled in [false, true] {
        for mut spec in [with_tap(), stable::stable()] {
            spec.checkout_lfs = enabled;
            let production = production::jobs(&spec).expect("production");
            let count = production
                .iter()
                .map(|(_, job)| checkout_count(job, enabled))
                .sum::<usize>();
            let expected_production_checkouts =
                if spec.policy.channel == PackageReleaseChannel::Preview {
                    2 // One matrix payload job and one aggregate checkout.
                } else {
                    spec.policy.payload_pairs().expect("pairs").len() + 1
                };
            assert_eq!(count, expected_production_checkouts);
            for (_, job) in [
                signer(&spec).expect("signer"),
                publisher(&spec).expect("publisher"),
                recovery(&spec).expect("recovery"),
            ] {
                assert_eq!(checkout_count(&job, enabled), 0);
            }
        }
        for mut spec in [with_tap(), stable_with_tap()] {
            spec.checkout_lfs = enabled;
            let tap = spec.homebrew_tap.as_ref().expect("tap");
            let contexts = if spec.policy.channel == PackageReleaseChannel::Preview {
                [
                    TapInvocationContext::Preview,
                    TapInvocationContext::Reconcile,
                ]
            } else {
                [
                    TapInvocationContext::StableAuthority,
                    TapInvocationContext::StableReconcile,
                ]
            };
            for context in contexts {
                let jobs = tap::jobs(&spec, tap, context).expect("tap jobs");
                assert_eq!(checkout_count(&jobs[0].1, enabled), 1);
                for (_, job) in &jobs[1..] {
                    assert_eq!(checkout_count(job, enabled), 0);
                }
            }
        }
    }
}

fn stable_with_tap() -> ConsumerReleaseSpec {
    let mut spec = stable::stable();
    let mut tap = with_tap().homebrew_tap.expect("tap");
    tap.policy.destinations.stable =
        Some(velnor_actions_contract::config::HomebrewTapDestination {
            formula: "Formula/scanner.rb".to_owned(),
        });
    tap.policy
        .updater
        .inputs
        .push("Formula/scanner.rb".to_owned());
    spec.homebrew_tap = Some(tap);
    spec
}
