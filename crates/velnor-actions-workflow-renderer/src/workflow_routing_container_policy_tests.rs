use velnor_actions_contract::ExecutionMode;

use super::{
    WorkflowRoutingPolicy, configured, route_workflow_yaml, runner_job, selector, workflow,
};
use crate::yaml::Yaml;

#[test]
fn selector_can_remain_containerless_without_a_configured_image() {
    let policy = WorkflowRoutingPolicy::configured(ExecutionMode::ScaleSet, selector(), None);
    let routed = route_workflow_yaml(
        "ci.yml",
        workflow(vec![("build", runner_job("ubuntu-26.04"))]),
        &policy,
    )
    .expect("A selector can use its already-configured host runtime.");
    let Yaml::Map(document) = routed else {
        panic!("workflow map")
    };
    let Some((_, Yaml::Map(jobs))) = document.iter().find(|(key, _)| key == "jobs") else {
        panic!("jobs map")
    };
    let Some((_, Yaml::Map(fields))) = jobs.first() else {
        panic!("job map")
    };
    assert!(fields.iter().any(|(key, value)| {
        key == "runs-on" && *value == Yaml::Flow(selector().labels().to_vec())
    }));
    assert!(!fields.iter().any(|(key, _)| key == "container"));
}

#[test]
fn an_explicit_container_is_still_validated_without_a_default_image() {
    let policy = WorkflowRoutingPolicy::configured(ExecutionMode::Hosted, selector(), None);
    let routed = route_workflow_yaml(
        "ci.yml",
        workflow(vec![(
            "build",
            Yaml::Map(vec![
                ("runs-on".to_owned(), Yaml::Flow(selector().labels().to_vec())),
                (
                    "container".to_owned(),
                    Yaml::str("ghcr.io/team/job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
                ),
            ]),
        )]),
        &policy,
    )
    .expect("An existing immutable container remains valid without a default image.");
    let Yaml::Map(document) = routed else {
        panic!("workflow map")
    };
    let Some((_, Yaml::Map(jobs))) = document.iter().find(|(key, _)| key == "jobs") else {
        panic!("jobs map")
    };
    let Some((_, Yaml::Map(fields))) = jobs.first() else {
        panic!("job map")
    };
    let Some((_, Yaml::Map(container))) = fields.iter().find(|(key, _)| key == "container") else {
        panic!("explicit container map")
    };
    assert!(container.iter().any(|(key, value)| {
        key == "image"
            && value
                == &Yaml::str(
                    "ghcr.io/team/job@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                )
    }));
    assert!(container.iter().any(|(key, _)| key == "volumes"));
}

#[test]
fn explicit_container_fields_are_preserved_and_validated() {
    let container = Yaml::Map(vec![
        (
            "image".to_owned(),
            Yaml::str(
                "ghcr.io/team/job@sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            ),
        ),
        (
            "credentials".to_owned(),
            Yaml::Map(vec![(
                "username".to_owned(),
                Yaml::str("${{ secrets.USER }}"),
            )]),
        ),
        (
            "env".to_owned(),
            Yaml::Map(vec![("MODE".to_owned(), Yaml::str("test"))]),
        ),
        ("ports".to_owned(), Yaml::Seq(vec![Yaml::str("6379")])),
        (
            "volumes".to_owned(),
            Yaml::Seq(vec![Yaml::str("data:/data")]),
        ),
        ("options".to_owned(), Yaml::str("--cpus 2")),
    ]);
    let original = workflow(vec![(
        "build",
        Yaml::Map(vec![
            (
                "runs-on".to_owned(),
                Yaml::Flow(selector().labels().to_vec()),
            ),
            ("container".to_owned(), container.clone()),
        ]),
    )]);
    let routed = route_workflow_yaml("ci.yml", original.clone(), &configured(ExecutionMode::Both))
        .expect("supported explicit container fields remain valid");
    let Yaml::Map(document) = routed else {
        panic!("workflow map")
    };
    let Some((_, Yaml::Map(jobs))) = document.iter().find(|(key, _)| key == "jobs") else {
        panic!("jobs map")
    };
    let Some((_, Yaml::Map(fields))) = jobs.first() else {
        panic!("job map")
    };
    let Some((_, Yaml::Map(container))) = fields.iter().find(|(key, _)| key == "container") else {
        panic!("container map")
    };
    assert!(container.iter().any(|(key, value)| {
        key == "volumes"
            && value
                == &Yaml::Seq(vec![
                    Yaml::str("data:/data"),
                    Yaml::str("/opt/velnor/job-facts:/opt/velnor/job-facts:ro"),
                    Yaml::str("/.velnor-runtime-identity:/.velnor-runtime-identity:ro"),
                ])
    }));
    assert!(
        container
            .iter()
            .any(|(key, value)| key == "options" && value == &Yaml::str("--cpus 2"))
    );
    let _ = original;
}
