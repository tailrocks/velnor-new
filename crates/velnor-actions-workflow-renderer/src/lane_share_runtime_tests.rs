use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind, StepRole};

use super::peel_cache_helper_prefix;

fn step(name: &str, role: Option<StepRole>) -> Step {
    Step {
        name: name.to_owned(),
        id: None,
        role,
        condition: None,
        kind: StepKind::Shell {
            run: vec!["true".to_owned()],
            env: BTreeMap::new(),
        },
    }
}

#[test]
fn preseed_failure_guard_stays_before_downloaded_helper_steps() {
    let guard = Step {
        name: "Require successful pre-seed plan".to_owned(),
        id: None,
        role: None,
        condition: Some("always() && needs.plan.result != 'success'".to_owned()),
        kind: StepKind::Shell {
            run: vec![
                "sh".to_owned(),
                "-c".to_owned(),
                crate::toolchain_env::with_credential_unset_script("exit 1"),
            ],
            env: crate::toolchain_env::credential_scrub(),
        },
    };
    let launcher = step(
        "Install cache tar adapter",
        Some(StepRole::CacheTarLaunchers),
    );
    let steps = vec![
        guard.clone(),
        step("Download", Some(StepRole::PreseedDownload)),
        step("Verify", Some(StepRole::PreseedVerifyManifest)),
        step("Stage", Some(StepRole::PreseedStage)),
        launcher.clone(),
        step("Setup Mise", Some(StepRole::MiseSetup)),
    ];

    let (prefix, remaining) = peel_cache_helper_prefix(&steps).expect("valid helper prefix");

    assert_eq!(&prefix[..1], std::slice::from_ref(&guard));
    assert_eq!(
        prefix
            .iter()
            .skip(1)
            .map(|step| step.role)
            .collect::<Vec<_>>(),
        [
            Some(StepRole::PreseedDownload),
            Some(StepRole::PreseedVerifyManifest),
            Some(StepRole::PreseedStage),
            Some(StepRole::CacheTarLaunchers),
        ]
    );
    assert_eq!(remaining, &steps[5..]);

    let local = vec![
        guard.clone(),
        steps[1].clone(),
        steps[2].clone(),
        steps[3].clone(),
    ];
    let (local_prefix, _) = peel_cache_helper_prefix(&local).expect("valid local prefix");
    assert!(super::same_cache_helper_prefix_shape(
        &prefix,
        &local_prefix,
        "ubuntu-26.04",
        "scale-set:velnor+ubuntu-26.04-scale-set",
        true,
        false,
    ));

    let mut malformed_guard = guard.clone();
    malformed_guard.condition = Some("success()".to_owned());
    let malformed = vec![
        malformed_guard.clone(),
        steps[1].clone(),
        steps[2].clone(),
        steps[3].clone(),
    ];
    let (malformed_prefix, malformed_remaining) =
        peel_cache_helper_prefix(&malformed).expect("unrecognized guard stays in the body");
    assert_eq!(malformed_prefix, [] as [Step; 0]);
    assert_eq!(malformed_remaining, malformed.as_slice());
    assert!(!super::same_cache_helper_prefix_shape(
        &prefix,
        &malformed_prefix,
        "ubuntu-26.04",
        "scale-set:velnor+ubuntu-26.04-scale-set",
        true,
        false,
    ));

    let reordered = vec![guard, steps[2].clone(), steps[1].clone(), steps[3].clone()];
    assert!(peel_cache_helper_prefix(&reordered).is_none());
}
