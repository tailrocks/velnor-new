use super::tests::{configured, image, selector, workflow};
use super::{WorkflowRoutingPolicy, route_workflow_yaml};
use crate::yaml::{AnchorName, Yaml};
use saphyr::LoadableYamlNode;
use velnor_actions_contract::ExecutionMode;

#[test]
fn hosted_and_both_preserve_existing_placement() {
    for mode in [ExecutionMode::Hosted, ExecutionMode::Both] {
        let input = workflow(vec![(
            "build",
            Yaml::Map(vec![("runs-on".to_owned(), Yaml::str("ubuntu-26.04"))]),
        )]);
        assert_eq!(
            route_workflow_yaml("ci.yml", input.clone(), &super::tests::configured(mode)),
            Ok(input)
        );
    }
}

fn container(value: Yaml) -> Yaml {
    workflow(vec![(
        "build",
        Yaml::Map(vec![
            (
                "runs-on".to_owned(),
                Yaml::Flow(selector().labels().to_vec()),
            ),
            ("container".to_owned(), value),
        ]),
    )])
}

#[test]
fn routing_rejects_private_mount_shadows_and_unresolved_mounts() {
    for (key, value) in [
        (
            "volumes",
            Yaml::Seq(vec![Yaml::str("/tmp:/opt/velnor/job-facts:ro")]),
        ),
        (
            "volumes",
            Yaml::Seq(vec![Yaml::str("/tmp:/.velnor-runtime-identity/child:ro")]),
        ),
        (
            "volumes",
            Yaml::Seq(vec![Yaml::str("/tmp:/opt/velnor/${{ inputs.path }}:ro")]),
        ),
        (
            "volumes",
            Yaml::Seq(vec![Yaml::str("${{ inputs.source }}:/work:ro")]),
        ),
        (
            "options",
            Yaml::str("--mount type=bind,destination=/opt/velnor/job-facts"),
        ),
        (
            "options",
            Yaml::str("--mount='type=bind,destination=/opt/velnor/job-facts'"),
        ),
        ("options", Yaml::str("-v /tmp:/.velnor-runtime-identity")),
        ("options", Yaml::str("--tmpfs /opt/velnor/job-facts")),
        ("options", Yaml::str("--volumes-from foreign")),
        ("options", Yaml::str("${{ inputs.container_options }}")),
        ("options", Yaml::str("--cpus 'unterminated")),
        ("options", Yaml::str("--env 'X=a --privileged --env A=B'")),
    ] {
        let routed = route_workflow_yaml(
            "ci.yml",
            container(Yaml::Map(vec![
                ("image".to_owned(), Yaml::str(image().as_str())),
                (key.to_owned(), value),
            ])),
            &configured(ExecutionMode::ScaleSet),
        );
        assert!(routed.is_err(), "accepted unsafe {key} value");
    }
}

#[test]
fn routing_accepts_only_proven_safe_docker_options() {
    let routed = route_workflow_yaml(
        "ci.yml",
        container(Yaml::Map(vec![
            ("image".to_owned(), Yaml::str(image().as_str())),
            (
                "options".to_owned(),
                Yaml::str("--cpus 2 --memory=1g --init"),
            ),
        ])),
        &configured(ExecutionMode::ScaleSet),
    )
    .expect("safe Docker options stay valid");
    let Yaml::Map(workflow) = routed else {
        panic!("workflow map")
    };
    let Some((_, Yaml::Map(jobs))) = workflow.iter().find(|(key, _)| key == "jobs") else {
        panic!("jobs map")
    };
    let Some((_, Yaml::Map(fields))) = jobs.first() else {
        panic!("job map")
    };
    let Some((_, Yaml::Map(container))) = fields.iter().find(|(key, _)| key == "container") else {
        panic!("container map")
    };
    assert!(container.iter().any(|(key, value)| {
        key == "options" && value == &Yaml::str("--cpus 2 --memory=1g --init")
    }));
}

#[test]
fn routing_preserves_supported_scalar_and_anchor_container_forms() {
    let anchor = AnchorName::new("runner").expect("anchor name");
    for source in [
        Yaml::str(image().as_str()),
        Yaml::AnchoredScalar {
            name: anchor,
            value: image().as_str().to_owned(),
        },
    ] {
        let routed = route_workflow_yaml(
            "ci.yml",
            container(source),
            &configured(ExecutionMode::ScaleSet),
        )
        .expect("valid scalar container form remains supported");
        let Yaml::Map(workflow) = routed else {
            panic!("workflow map")
        };
        let Some((_, Yaml::Map(jobs))) = workflow.iter().find(|(key, _)| key == "jobs") else {
            panic!("jobs map")
        };
        let Some((_, Yaml::Map(fields))) = jobs.first() else {
            panic!("job map")
        };
        let Some((_, routed_container)) = fields.iter().find(|(key, _)| key == "container") else {
            panic!("container field")
        };
        let (Yaml::Map(entries) | Yaml::AnchoredMap { entries, .. }) = routed_container else {
            panic!("container map")
        };
        assert!(entries.iter().any(|(key, _)| key == "image"));
        assert!(entries.iter().any(|(key, _)| key == "volumes"));
    }
}

#[test]
fn scalar_anchor_stays_bound_to_the_image_after_rendering() {
    let anchor = AnchorName::new("runner_image").expect("anchor name");
    let routed = route_workflow_yaml(
        "ci.yml",
        workflow(vec![(
            "build",
            Yaml::Map(vec![
                (
                    "runs-on".to_owned(),
                    Yaml::Flow(selector().labels().to_vec()),
                ),
                (
                    "container".to_owned(),
                    Yaml::AnchoredScalar {
                        name: anchor.clone(),
                        value: image().as_str().to_owned(),
                    },
                ),
                (
                    "outputs".to_owned(),
                    Yaml::Map(vec![("image".to_owned(), Yaml::Alias(anchor))]),
                ),
            ]),
        )]),
        &configured(ExecutionMode::ScaleSet),
    )
    .expect("scalar image anchor stays valid");
    let rendered = crate::yaml::render_yaml(&routed);
    let parsed = saphyr::Yaml::load_from_str(&rendered).expect("rendered YAML parses");
    assert_eq!(
        parsed[0]["jobs"]["build"]["outputs"]["image"].as_str(),
        Some(image().as_str())
    );
}

#[test]
fn shared_container_map_keeps_hosted_alias_unmodified_in_both_mode() {
    let anchor = AnchorName::new("runner_container").expect("anchor name");
    let routed = route_workflow_yaml(
        "ci.yml",
        workflow(vec![
            (
                "build",
                Yaml::Map(vec![
                    (
                        "runs-on".to_owned(),
                        Yaml::Flow(selector().labels().to_vec()),
                    ),
                    (
                        "container".to_owned(),
                        Yaml::AnchoredMap {
                            name: anchor.clone(),
                            entries: vec![("image".to_owned(), Yaml::str(image().as_str()))],
                        },
                    ),
                ]),
            ),
            (
                "hosted",
                Yaml::Map(vec![
                    ("runs-on".to_owned(), Yaml::str("ubuntu-26.04")),
                    ("container".to_owned(), Yaml::Alias(anchor)),
                ]),
            ),
        ]),
        &configured(ExecutionMode::Both),
    )
    .expect("both mode keeps hosted alias separate");
    let rendered = crate::yaml::render_yaml(&routed);
    let parsed = saphyr::Yaml::load_from_str(&rendered).expect("rendered YAML parses");
    let jobs = parsed[0]["jobs"].as_mapping().expect("jobs mapping");
    let hosted = jobs
        .iter()
        .find(|(key, _)| key.as_str() == Some("hosted"))
        .map(|(_, value)| value)
        .expect("hosted job remains");
    let container = hosted["container"]
        .as_mapping()
        .expect("hosted container mapping");
    assert_eq!(container.len(), 1);
    let (key, value) = container.iter().next().expect("image field");
    assert_eq!(key.as_str(), Some("image"));
    assert_eq!(value.as_str(), Some(image().as_str()));
}

#[test]
fn routing_preserves_explicit_anchored_container_maps() {
    let anchor = AnchorName::new("container").expect("anchor name");
    let source = Yaml::AnchoredMap {
        name: anchor,
        entries: vec![("image".to_owned(), Yaml::str(image().as_str()))],
    };
    let routed = route_workflow_yaml(
        "ci.yml",
        container(source),
        &WorkflowRoutingPolicy::configured(ExecutionMode::ScaleSet, selector(), Some(image())),
    )
    .expect("anchored map remains supported");
    let Yaml::Map(workflow) = routed else {
        panic!("workflow map")
    };
    let Some((_, Yaml::Map(jobs))) = workflow.iter().find(|(key, _)| key == "jobs") else {
        panic!("jobs map")
    };
    let Some((_, Yaml::Map(fields))) = jobs.first() else {
        panic!("job map")
    };
    let Some((_, Yaml::Map(entries))) = fields.iter().find(|(key, _)| key == "container") else {
        panic!("materialized container map")
    };
    assert!(entries.iter().any(|(key, value)| {
        key == "volumes"
            && value
                == &Yaml::Seq(vec![
                    Yaml::str("/opt/velnor/job-facts:/opt/velnor/job-facts:ro"),
                    Yaml::str("/.velnor-runtime-identity:/.velnor-runtime-identity:ro"),
                ])
    }));
}
