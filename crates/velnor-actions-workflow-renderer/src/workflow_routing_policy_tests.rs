use super::*;

#[test]
fn hosted_containers_remain_unchanged_in_hosted_only_mode() {
    let input = workflow(vec![(
        "build",
        Yaml::Map(vec![
            ("runs-on".to_owned(), Yaml::str("ubuntu-26.04")),
            ("container".to_owned(), Yaml::str("ubuntu:24.04")),
        ]),
    )]);
    assert_eq!(
        route_workflow_yaml(
            "ci.yml",
            input.clone(),
            &WorkflowRoutingPolicy::hosted_only()
        ),
        Ok(input)
    );
}

#[test]
fn unconfigured_selector_leaf_fails_closed() {
    let error = route_workflow_yaml(
        "ci.yml",
        workflow(vec![(
            "build",
            Yaml::Map(vec![(
                "runs-on".to_owned(),
                Yaml::Flow(selector().labels().to_vec()),
            )]),
        )]),
        &WorkflowRoutingPolicy::hosted_only(),
    )
    .expect_err("an unconfigured Scale Set selector cannot run");
    assert!(matches!(error, RenderError::InvalidWorkflow(_)));
}

#[test]
fn untyped_custom_hosted_label_fails_closed() {
    let input = workflow(vec![(
        "check-external",
        Yaml::Map(vec![("runs-on".to_owned(), Yaml::str("native-scale-set"))]),
    )]);
    assert!(route_workflow_yaml("ci.yml", input, &WorkflowRoutingPolicy::hosted_only()).is_err());
}

#[test]
fn foreign_flow_selector_fails_in_scale_set_mode() {
    let error = route_workflow_yaml(
        "ci.yml",
        workflow(vec![(
            "build",
            Yaml::Map(vec![(
                "runs-on".to_owned(),
                Yaml::Flow(vec!["velnor".to_owned(), "other".to_owned()]),
            )]),
        )]),
        &configured(ExecutionMode::ScaleSet),
    )
    .expect_err("a foreign selector cannot pass the Scale Set routing gate");
    assert!(matches!(error, RenderError::InvalidWorkflow(_)));
}

#[test]
fn injected_container_keeps_workflow_shell() {
    let input = Yaml::Map(vec![
        (
            "defaults".to_owned(),
            Yaml::Map(vec![(
                "run".to_owned(),
                Yaml::Map(vec![("shell".to_owned(), Yaml::str("pwsh"))]),
            )]),
        ),
        (
            "jobs".to_owned(),
            Yaml::Map(vec![("build".to_owned(), runner_job("ubuntu-26.04"))]),
        ),
    ]);
    let routed = route_workflow_yaml("ci.yml", input, &configured(ExecutionMode::ScaleSet))
        .expect("configured workflow shell stays effective in the container");
    let Yaml::Map(document) = routed else {
        panic!("workflow map")
    };
    let (_, Yaml::Map(jobs)) = document
        .iter()
        .find(|(key, _)| key == "jobs")
        .expect("workflow structure")
    else {
        panic!("jobs map")
    };
    let (_, Yaml::Map(job)) = &jobs[0] else {
        panic!("job map")
    };
    let (_, Yaml::Map(defaults)) = job
        .iter()
        .find(|(key, _)| key == "defaults")
        .expect("workflow structure")
    else {
        panic!("job defaults")
    };
    let (_, Yaml::Map(run)) = defaults
        .iter()
        .find(|(key, _)| key == "run")
        .expect("workflow structure")
    else {
        panic!("run defaults")
    };
    assert!(
        run.iter()
            .any(|(key, value)| key == "shell" && value == &Yaml::str("pwsh"))
    );
}
