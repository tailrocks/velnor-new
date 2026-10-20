//! Clippy-only and configuration-wide all-features task derivation.

use super::{inputs, nextest_profile, package};
use velnor_actions_rust::{DeriveInputs, TaskKind, derive_task_groups, tasks::cargo_payload_argv};

#[test]
fn clippy_feature_policy_changes_only_clippy_and_keeps_fmt_deterministic() {
    let package = package();
    let profile = nextest_profile();
    let features = vec!["default".to_owned()];
    let groups = derive_task_groups(&DeriveInputs {
        package: &package,
        profile: &profile,
        configuration: "default",
        features: &features,
        all_features: false,
        clippy_all_features: true,
        target: "host",
        explicit_fmt: true,
    })
    .expect("Task derivation must accept the Clippy feature policy.");
    assert_eq!(
        groups,
        derive_task_groups(&DeriveInputs {
            package: &package,
            profile: &profile,
            configuration: "default",
            features: &features,
            all_features: false,
            clippy_all_features: true,
            target: "host",
            explicit_fmt: true,
        })
        .expect("Task derivation must be deterministic.")
    );
    assert!(groups.iter().any(|group| group.kind == TaskKind::Fmt));

    for group in &groups {
        let argv = cargo_payload_argv(group)
            .expect("Task arguments must be available.")
            .into_iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        match group.kind {
            TaskKind::Clippy => assert!(argv.iter().any(|arg| arg == "--all-features")),
            TaskKind::Fmt => {
                assert!(
                    argv.contains(&"fmt".to_owned()),
                    "The arguments must include fmt. Arguments: {argv:?}."
                );
                assert!(!argv.iter().any(|arg| arg == "--all-features"));
            }
            TaskKind::Build | TaskKind::Nextest | TaskKind::Doctest | TaskKind::Doc => {
                assert!(!argv.iter().any(|arg| arg == "--all-features"));
            }
            TaskKind::Test => panic!("The task list must not have a Test task."),
        }
    }
}

#[test]
fn global_and_default_feature_policies_keep_their_existing_argv() {
    let package = package();
    let profile = nextest_profile();
    let features = vec!["default".to_owned()];
    let default_inputs = inputs(&package, &profile, &features);
    let default_groups =
        derive_task_groups(&default_inputs).expect("The default task derivation must succeed.");
    assert_eq!(
        default_groups,
        derive_task_groups(&default_inputs)
            .expect("Repeated default task derivation must succeed.")
    );
    for group in &default_groups {
        assert!(
            !cargo_payload_argv(group)
                .expect("Default task arguments must be available.")
                .iter()
                .any(|arg| arg == "--all-features"),
            "The default {} task must not include --all-features.",
            group.kind.as_str()
        );
    }

    let globally_enabled = derive_task_groups(&DeriveInputs {
        package: &package,
        profile: &profile,
        configuration: "global",
        features: &[],
        all_features: true,
        clippy_all_features: false,
        target: "host",
        explicit_fmt: true,
    })
    .expect("The global all-features task derivation must succeed.");
    for group in &globally_enabled {
        let argv = cargo_payload_argv(group).expect("Task arguments must be available.");
        assert_eq!(
            argv.iter().any(|arg| arg == "--all-features"),
            group.kind != TaskKind::Fmt,
            "The global policy must set --all-features for {}.",
            group.kind.as_str()
        );
    }
}
