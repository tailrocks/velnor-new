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
        for minutes in [5, 1, 15, 333] {
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

fn job_timeout(job: &Yaml) -> i64 {
    let Yaml::Map(fields) = job else {
        panic!("job mapping")
    };
    let (_, Yaml::Int(timeout)) = fields
        .iter()
        .find(|(key, _)| key == "timeout-minutes")
        .expect("job timeout")
    else {
        panic!("literal job timeout")
    };
    *timeout
}

fn step_timeouts(job: &Yaml) -> Vec<i64> {
    let Yaml::Map(fields) = job else {
        panic!("job mapping")
    };
    let Some((_, Yaml::Seq(steps))) = fields.iter().find(|(key, _)| key == "steps") else {
        panic!("job steps")
    };
    steps
        .iter()
        .filter_map(|step| {
            let Yaml::Map(fields) = step else {
                panic!("step mapping")
            };
            fields.iter().find_map(|(key, value)| {
                if key != "timeout-minutes" {
                    return None;
                }
                let Yaml::Int(minutes) = value else {
                    panic!("literal step timeout")
                };
                Some(*minutes)
            })
        })
        .collect()
}

#[test]
fn publication_jobs_budget_every_preparation_step_and_preserve_acquisition_cap() {
    for mut spec in [fixture(), stable::stable()] {
        spec.publication_timeout_minutes = JobTimeout::new(15).expect("bounded budget");
        let (publisher_id, publisher) = super::publisher(&spec).expect("publisher");
        assert!(publisher_id.ends_with("-publisher"));
        assert_eq!(job_timeout(&publisher), 42);
        assert_eq!(step_timeouts(&publisher), [5, 5, 6, 5, 5]);

        let (recovery_id, recovery) = super::recovery(&spec).expect("recovery");
        assert!(recovery_id.ends_with("-recovery"));
        assert_eq!(job_timeout(&recovery), 32);
        assert_eq!(step_timeouts(&recovery), [5, 5, 6]);
    }
}

#[test]
fn publisher_budget_tracks_a_changed_preparation_step_cap() {
    let mut spec = fixture();
    spec.publication_timeout_minutes = JobTimeout::new(15).expect("bounded budget");
    let (_, job) = super::publisher(&spec).expect("publisher");
    let Yaml::Map(mut fields) = job else {
        panic!("publisher job mapping")
    };
    let Some(Yaml::Seq(mut steps)) = fields
        .iter()
        .find(|(key, _)| key == "steps")
        .map(|(_, steps)| steps.clone())
    else {
        panic!("publisher job steps")
    };
    let Yaml::Map(first_step) = &mut steps[0] else {
        panic!("setup step mapping")
    };
    let Some((_, cap)) = first_step
        .iter_mut()
        .find(|(key, _)| key == "timeout-minutes")
    else {
        panic!("setup step cap")
    };
    *cap = Yaml::Int(6);
    super::super::publication_budget::apply_preparation_budget(
        &mut fields,
        &mut steps[..5],
        spec.publication_timeout_minutes,
    )
    .expect("budget follows rendered caps");
    assert_eq!(job_timeout(&Yaml::Map(fields)), 43);
}

#[test]
fn maximum_publication_runtime_fits_publisher_and_over_limit_is_rejected() {
    let mut preview = fixture();
    preview.publication_timeout_minutes = JobTimeout::new(333).expect("maximum runtime");
    assert!(render_consumer_preview(&preview).is_ok());
    let (_, preview_publisher) = super::publisher(&preview).expect("preview publisher");
    assert_eq!(job_timeout(&preview_publisher), 360);

    let mut stable_maximum = stable::stable();
    stable_maximum.publication_timeout_minutes = JobTimeout::new(333).expect("maximum runtime");
    assert!(render_consumer_binary_release(&stable_maximum).is_ok());
    let (_, stable_publisher) = super::publisher(&stable_maximum).expect("stable publisher");
    assert_eq!(job_timeout(&stable_publisher), 360);

    preview.publication_timeout_minutes = JobTimeout::new(334).expect("representable runtime");
    assert!(render_consumer_preview(&preview).is_err());
    let mut stable = stable::stable();
    stable.publication_timeout_minutes = JobTimeout::new(334).expect("representable runtime");
    assert!(render_consumer_binary_release(&stable).is_err());
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
    let preview = render_consumer_preview(&spec).expect("preview").bytes;
    assert_eq!(preview.matches(BUDGET_ENV).count(), 2);
    let stable = render_consumer_binary_release(&stable::stable())
        .expect("stable")
        .bytes;
    assert_eq!(stable.matches(BUDGET_ENV).count(), 2);
    let reconcile = render_release_tap_reconcile(&spec)
        .expect("reconciliation")
        .bytes;
    assert!(!reconcile.contains(BUDGET_ENV));
}
