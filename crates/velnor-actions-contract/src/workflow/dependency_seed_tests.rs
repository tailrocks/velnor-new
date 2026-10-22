use super::{cache_role, valid_preseed_consumer_steps};
use crate::workflow::{Step, StepKind, StepRole};
use std::collections::BTreeMap;

fn steps(roles: &[StepRole]) -> Vec<Step> {
    roles
        .iter()
        .copied()
        .map(|role| Step {
            name: format!("{role:?}"),
            id: None,
            role: Some(role),
            condition: None,
            parallel: false,
            kind: StepKind::Shell {
                run: vec!["true".to_owned()],
                env: BTreeMap::new(),
            },
        })
        .collect()
}

#[test]
fn preseed_consumer_lifecycle_is_optional_or_exactly_ordered_before_consumers() {
    assert!(valid_preseed_consumer_steps(&steps(&[
        StepRole::Checkout,
        StepRole::MbxPreflight,
    ])));
    assert!(valid_preseed_consumer_steps(&steps(&[
        StepRole::Checkout,
        StepRole::PreseedDownload,
        StepRole::PreseedVerifyManifest,
        StepRole::PreseedStage,
        StepRole::CacheTarLaunchers,
        StepRole::PrivateFetch,
        StepRole::MbxSeedBuild,
    ])));
}

#[test]
fn preseed_consumer_lifecycle_rejects_partial_duplicate_or_reordered_steps() {
    for roles in [
        vec![StepRole::Checkout, StepRole::PreseedDownload],
        vec![
            StepRole::Checkout,
            StepRole::PreseedDownload,
            StepRole::PreseedDownload,
            StepRole::PreseedVerifyManifest,
            StepRole::PreseedStage,
        ],
        vec![
            StepRole::Checkout,
            StepRole::PreseedVerifyManifest,
            StepRole::PreseedDownload,
            StepRole::PreseedStage,
        ],
    ] {
        assert!(!valid_preseed_consumer_steps(&steps(&roles)), "{roles:?}");
    }
}

#[test]
fn preseed_consumer_lifecycle_requires_checkout_and_precedes_helper_consumers() {
    for roles in [
        vec![
            StepRole::PreseedDownload,
            StepRole::PreseedVerifyManifest,
            StepRole::PreseedStage,
        ],
        vec![
            StepRole::Checkout,
            StepRole::PrivateFetch,
            StepRole::PreseedDownload,
            StepRole::PreseedVerifyManifest,
            StepRole::PreseedStage,
        ],
        vec![
            StepRole::Checkout,
            StepRole::PreseedDownload,
            StepRole::PreseedVerifyManifest,
            StepRole::PreseedStage,
            StepRole::AcquireVelnor,
        ],
    ] {
        assert!(!valid_preseed_consumer_steps(&steps(&roles)), "{roles:?}");
    }
}

#[test]
fn preseed_producer_roles_remain_outside_the_closed_seed_cache_role_set() {
    for role in [
        StepRole::PreseedBuild,
        StepRole::PreseedManifest,
        StepRole::PreseedUpload,
    ] {
        assert!(!cache_role(role), "{role:?}");
    }
}
