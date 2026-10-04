use super::{DesktopGraphSteps, cadence, release};
use std::collections::BTreeMap;
use velnor_actions_contract::config::{DesktopDeliveryConfig, NativeDesktopProfile};
use velnor_actions_contract::workflow::{PermissionLevel, Step, StepKind};

fn config() -> DesktopDeliveryConfig {
    DesktopDeliveryConfig {
        enabled: true,
        repository: "example/orbit".to_owned(),
        profile: Some(
            serde_json::from_str::<NativeDesktopProfile>(
                r#"{
                    "ffi": {
                        "manifest_path": "bridge/Cargo.toml", "package": "orbit-bridge",
                        "profile": "release", "framework_name": "OrbitCore", "module_name": "OrbitCoreFFI",
                        "static_library": "liborbit_bridge.a", "bindings_path": "native/Generated/Bindings",
                        "xcframework_path": "build/OrbitCore.xcframework"
                    },
                    "native_root": "native", "deployment_target": "26.0",
                    "apple": {
                        "project_spec": "application.yml", "project_path": "Orbit.xcodeproj",
                        "scheme": "Orbit", "app_name": "Orbit",
                        "bundle_identifier": "org.example.orbit", "bundle_name": "Orbit",
                        "app_path": "build/Orbit.app", "derived_data_path": "build/data",
                        "archive_name_prefix": "orbit-desktop"
                    }
                }"#,
            )
            .expect("valid desktop profile fixture"),
        ),
        ..DesktopDeliveryConfig::default()
    }
}

fn checkout() -> Step {
    Step {
        id: None,
        name: "Check out source".to_owned(),
        condition: None,
        kind: StepKind::Action {
            uses: "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1".to_owned(),
            with: BTreeMap::from([
                ("persist-credentials".to_owned(), "false".to_owned()),
                ("fetch-depth".to_owned(), "0".to_owned()),
                ("ref".to_owned(), "${{ github.sha }}".to_owned()),
            ]),
            env: BTreeMap::new(),
        },
    }
}

#[test]
fn unsigned_release_has_dispatch_only_and_no_publication_authority() {
    let graph = release(
        &config(),
        "trunk",
        DesktopGraphSteps {
            unsigned: vec![checkout()],
        },
    )
    .expect("unsigned release graph");
    graph.validate().expect("valid neutral graph");
    assert!(graph.triggers.push_tags.is_empty());
    assert!(graph.triggers.push_branches.is_empty());
    assert!(graph.triggers.schedule.is_none());
    let dispatch = graph.triggers.workflow_dispatch.expect("manual dispatch");
    assert_eq!(dispatch.inputs.len(), 1);
    assert_eq!(dispatch.inputs[0].name, "mode");
    assert_eq!(dispatch.inputs[0].options, ["validate"]);
    assert_eq!(dispatch.inputs[0].default.as_deref(), Some("validate"));
    assert_eq!(graph.jobs.len(), 1);
    let build = graph.jobs.get("build").expect("unsigned build job");
    assert!(build.needs.is_empty());
    assert!(build.environment.is_none());
    assert_eq!(build.steps, [checkout()]);
    assert!(build.condition.as_deref().is_some_and(|condition| {
        condition.contains("github.repository == 'example/orbit'")
            && condition.contains("github.event_name == 'workflow_dispatch'")
            && condition.contains("inputs.mode == 'validate'")
    }));
    for permissions in [Some(&graph.permissions), build.permissions.as_ref()]
        .into_iter()
        .flatten()
    {
        assert_ne!(permissions.contents, PermissionLevel::Write);
        assert_ne!(permissions.id_token, PermissionLevel::Write);
        assert_ne!(permissions.actions, PermissionLevel::Write);
        assert_ne!(permissions.pages, PermissionLevel::Write);
    }
}

#[test]
fn merge_and_scheduled_cadences_keep_distinct_events_and_literal_branch_guard() {
    for scheduled in [false, true] {
        let graph = cadence(&config(), "trunk", scheduled, vec![checkout()])
            .expect("desktop cadence graph");
        graph.validate().expect("valid neutral graph");
        assert!(graph.triggers.push_tags.is_empty());
        assert_eq!(
            graph.triggers.push_branches,
            if scheduled {
                Vec::<String>::new()
            } else {
                vec!["trunk".to_owned()]
            }
        );
        assert_eq!(graph.triggers.schedule.is_some(), scheduled);
        if scheduled {
            assert_eq!(
                graph.triggers.schedule.as_ref().expect("cron").cron,
                ["41 4 * * 1"]
            );
        }
        assert!(graph.triggers.workflow_dispatch.is_some());
        assert_eq!(graph.jobs.len(), 1);
        let id = if scheduled {
            "desktop-scheduled"
        } else {
            "desktop-merge"
        };
        let kind = if scheduled { "scheduled" } else { "merge" };
        assert_eq!(
            graph.run_name.as_deref(),
            Some(format!("Desktop {kind} cadence · ${{{{ github.event_name }}}}").as_str())
        );
        let job = graph.jobs.get(id).expect("cadence job");
        let condition = job.condition.as_deref().expect("bound job condition");
        assert!(condition.contains("github.repository == 'example/orbit'"));
        assert!(condition.contains("github.ref == 'refs/heads/trunk'"));
        assert!(condition.contains("github.event.repository.default_branch"));
        assert!(condition.contains(if scheduled {
            "github.event_name == 'schedule'"
        } else {
            "github.event_name == 'push'"
        }));
        assert!(condition.contains("github.event_name == 'workflow_dispatch'"));
        assert!(job.environment.is_none());
        assert_eq!(job.steps, [checkout()]);
        assert_ne!(graph.permissions.contents, PermissionLevel::Write);
        assert_ne!(graph.permissions.id_token, PermissionLevel::Write);
    }
}

#[test]
fn unsafe_default_branch_never_enters_release_or_cadence_expressions() {
    for branch in [
        "bad' || always() || '",
        "${{ github.ref }}",
        "../trunk",
        "trunk\nother",
    ] {
        assert!(
            release(
                &config(),
                branch,
                DesktopGraphSteps {
                    unsigned: vec![checkout()]
                }
            )
            .is_err(),
            "release accepted {branch:?}"
        );
        for scheduled in [false, true] {
            assert!(
                cadence(&config(), branch, scheduled, vec![checkout()]).is_err(),
                "cadence accepted {branch:?}"
            );
        }
    }
}

#[test]
fn signed_release_fails_closed_without_qualified_signing_role() {
    let mut signed = config();
    signed.sign_tags = true;
    signed.certificate_sha256 = Some("a".repeat(64));
    signed.team_id = Some("ABCDE12345".to_owned());
    let error = release(
        &signed,
        "trunk",
        DesktopGraphSteps {
            unsigned: vec![checkout()],
        },
    )
    .expect_err("signed graph needs closed authority");
    assert!(
        error
            .to_string()
            .contains("desktop_signing_authority_unqualified")
    );
}

#[test]
fn desktop_graph_requires_enabled_policy_and_owner_prepared_steps() {
    let mut disabled = config();
    disabled.enabled = false;
    let error = release(
        &disabled,
        "trunk",
        DesktopGraphSteps {
            unsigned: vec![checkout()],
        },
    )
    .expect_err("disabled desktop graph");
    assert!(error.to_string().contains("desktop_graph_disabled"));

    let shell = Step {
        id: None,
        name: "Unqualified shell".to_owned(),
        condition: None,
        kind: StepKind::Shell {
            run: vec!["echo".to_owned(), "unsafe".to_owned()],
            env: BTreeMap::new(),
        },
    };
    let error = release(
        &config(),
        "trunk",
        DesktopGraphSteps {
            unsigned: vec![shell],
        },
    )
    .expect_err("raw shell entered desktop graph");
    assert!(
        error
            .to_string()
            .contains("desktop_requires_owner_operations")
    );
    let error = cadence(&config(), "trunk", false, Vec::new()).expect_err("empty desktop cadence");
    assert!(
        error
            .to_string()
            .contains("desktop_requires_owner_operations")
    );
}

fn reject_unqualified_action(step: Step) {
    let error = release(
        &config(),
        "trunk",
        DesktopGraphSteps {
            unsigned: vec![step.clone()],
        },
    )
    .expect_err("unqualified release action");
    assert!(
        error
            .to_string()
            .contains("desktop_requires_owner_operations")
    );
    for scheduled in [false, true] {
        let error = cadence(&config(), "trunk", scheduled, vec![step.clone()])
            .expect_err("unqualified cadence action");
        assert!(
            error
                .to_string()
                .contains("desktop_requires_owner_operations")
        );
    }
}

#[test]
fn arbitrary_actions_and_checkout_authority_mutations_are_rejected() {
    let mut arbitrary = checkout();
    if let StepKind::Action { uses, .. } = &mut arbitrary.kind {
        *uses = format!("actions/setup-node@{}", "b".repeat(40));
    }
    reject_unqualified_action(arbitrary);

    let mut alternate_ref = checkout();
    if let StepKind::Action { with, .. } = &mut alternate_ref.kind {
        with.insert("ref".to_owned(), "${{ github.ref }}".to_owned());
    }
    reject_unqualified_action(alternate_ref);

    let mut credentials = checkout();
    if let StepKind::Action { with, .. } = &mut credentials.kind {
        with.insert("persist-credentials".to_owned(), "true".to_owned());
    }
    reject_unqualified_action(credentials);

    let mut environment = checkout();
    if let StepKind::Action { env, .. } = &mut environment.kind {
        env.insert("GH_TOKEN".to_owned(), "${{ secrets.TOKEN }}".to_owned());
    }
    reject_unqualified_action(environment);

    let mut conditional = checkout();
    conditional.condition = Some("always()".to_owned());
    reject_unqualified_action(conditional);
}

#[test]
fn duplicate_checkout_cannot_reenter_source_after_owner_preparation() {
    for scheduled in [false, true] {
        assert!(cadence(&config(), "trunk", scheduled, vec![checkout(), checkout()]).is_err());
    }
    assert!(
        release(
            &config(),
            "trunk",
            DesktopGraphSteps {
                unsigned: vec![checkout(), checkout()]
            }
        )
        .is_err()
    );
}
