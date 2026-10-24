use super::*;
use crate::schema2::render_consumer_binary_release;
use velnor_actions_contract::JobTimeout;

const BUDGET_ENV: &str = "VELNOR_RELEASE_PUBLICATION_TIMEOUT_MINUTES";

fn budget_values(step: &Yaml) -> Vec<&Yaml> {
    let Yaml::Map(fields) = step else {
        panic!("operation step mapping")
    };
    let (_, Yaml::Map(env)) = fields
        .iter()
        .find(|(key, _)| key == "env")
        .expect("operation environment")
    else {
        panic!("environment mapping")
    };
    env.iter()
        .filter(|(key, _)| key == BUDGET_ENV)
        .map(|(_, value)| value)
        .collect()
}

#[test]
fn publication_operations_bind_one_canonical_configured_budget_in_both_channels() {
    for mut spec in [fixture(), stable::stable()] {
        let context = TapInvocationContext::for_channel(spec.policy.channel);
        for minutes in [5, 1, 15, 360] {
            spec.publication_timeout_minutes = JobTimeout::new(minutes).expect("bounded budget");
            for op in ["publish-package-release-v1", "recover-package-release-v1"] {
                let step = steps::operation(&spec, op, true, context).expect("publication step");
                assert_eq!(
                    budget_values(&step),
                    [&Yaml::str(minutes.to_string())],
                    "{op} receives one literal configured budget"
                );
            }
        }
    }
}

#[test]
fn publication_budget_is_absent_from_other_native_operations_and_reconciliation() {
    for mut spec in [fixture(), stable::stable()] {
        spec.publication_timeout_minutes = JobTimeout::new(15).expect("bounded budget");
        let context = TapInvocationContext::for_channel(spec.policy.channel);
        for op in [
            "select-preview-source-v1",
            "finalize-preview-source-v1",
            "acquire-preview-source-v1",
            "execute-release-producer-v1",
            "acquire-release-leaves-v1",
            "admit-release-aggregate-v1",
            "sign-package-release-v1",
            "write-stable-tag-intent-v1",
            "write-stable-tag-origin-v1",
            "select-stable-release-source-v1",
            "admit-stable-tag-origin-v1",
            "admit-homebrew-tap-v1",
            "prepare-homebrew-tap-v1",
            "write-homebrew-tap-pr-v1",
            "merge-homebrew-tap-pr-v1",
            "select-release-tap-continuation-v1",
        ] {
            let step = steps::operation(&spec, op, true, context).expect("native step");
            assert!(budget_values(&step).is_empty(), "{op} keeps its own budget");
        }
    }
    let mut spec = with_tap();
    spec.publication_timeout_minutes = JobTimeout::new(15).expect("bounded budget");
    let preview = render_consumer_preview(&spec, &hosted_policy())
        .expect("preview")
        .bytes;
    assert_eq!(preview.matches(BUDGET_ENV).count(), 2);
    let stable = render_consumer_binary_release(&stable::stable(), &hosted_policy())
        .expect("stable")
        .bytes;
    assert_eq!(stable.matches(BUDGET_ENV).count(), 2);
    let reconcile = render_release_tap_reconcile(&spec, &hosted_policy())
        .expect("reconciliation")
        .bytes;
    assert!(!reconcile.contains(BUDGET_ENV));
}
