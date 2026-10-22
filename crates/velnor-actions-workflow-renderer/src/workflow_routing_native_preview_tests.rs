use super::super::route_native_preview_workflow_yaml;
use super::*;
use crate::yaml::render_yaml;
use saphyr::LoadableYamlNode;
use velnor_actions_contract::{ExecutionMode, ScaleSetSelector};

fn selector() -> ScaleSetSelector {
    ScaleSetSelector::try_new(
        "linux-amd64",
        &["velnor".to_owned(), "linux-amd64".to_owned()],
    )
    .expect("valid selector")
}

fn workflow(require_linux_amd64: bool) -> Yaml {
    let mut source_env = vec![("VELNOR_INTERNAL_OP".to_owned(), Yaml::str(SOURCE_OPERATION))];
    if require_linux_amd64 {
        source_env.push((
            "VELNOR_RELEASE_REQUIRE_LINUX_AMD64".to_owned(),
            Yaml::str("true"),
        ));
    }
    Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![
            (
                SOURCE_JOB.to_owned(),
                Yaml::Map(vec![
                    ("runs-on".to_owned(), Yaml::str("ubuntu-26.04")),
                    (
                        "outputs".to_owned(),
                        Yaml::Map(vec![
                            (
                                "payload_matrix".to_owned(),
                                Yaml::str("${{ steps.source.outputs.payload_matrix }}"),
                            ),
                            (
                                "producer_runner".to_owned(),
                                Yaml::str("${{ steps.source.outputs.producer_runner }}"),
                            ),
                        ]),
                    ),
                    (
                        "steps".to_owned(),
                        Yaml::Seq(vec![Yaml::Map(vec![
                            ("id".to_owned(), Yaml::str(SOURCE_STEP)),
                            ("env".to_owned(), Yaml::Map(source_env)),
                        ])]),
                    ),
                ]),
            ),
            (
                PAYLOAD_JOB.to_owned(),
                Yaml::Map(vec![
                    ("runs-on".to_owned(), Yaml::str(PAYLOAD_RUNNER)),
                    ("needs".to_owned(), Yaml::Seq(vec![Yaml::str(SOURCE_JOB)])),
                    (
                        "strategy".to_owned(),
                        Yaml::Map(vec![
                            ("fail-fast".to_owned(), Yaml::Bool(false)),
                            ("matrix".to_owned(), Yaml::str(PAYLOAD_MATRIX)),
                        ]),
                    ),
                ]),
            ),
            (
                ASSEMBLE_JOB.to_owned(),
                Yaml::Map(vec![
                    ("runs-on".to_owned(), Yaml::str(ASSEMBLE_RUNNER)),
                    (
                        "needs".to_owned(),
                        Yaml::Seq(vec![Yaml::str(SOURCE_JOB), Yaml::str(PAYLOAD_JOB)]),
                    ),
                ]),
            ),
        ]),
    )])
}

fn field_mut<'a>(workflow: &'a mut Yaml, job_id: &str, key: &str) -> &'a mut Yaml {
    let Yaml::Map(document) = workflow else {
        panic!("workflow map")
    };
    let Some((_, Yaml::Map(jobs))) = document.iter_mut().find(|(name, _)| name == "jobs") else {
        panic!("jobs map")
    };
    let Some((_, Yaml::Map(fields))) = jobs.iter_mut().find(|(name, _)| name == job_id) else {
        panic!("job map")
    };
    let Some((_, value)) = fields.iter_mut().find(|(name, _)| name == key) else {
        panic!("job field")
    };
    value
}

fn policy(mode: ExecutionMode) -> WorkflowRoutingPolicy {
    WorkflowRoutingPolicy::configured(mode, selector(), None)
}

#[test]
fn native_preview_contract_preserves_dynamic_runner_matrix_for_hosted_and_both() {
    for policy in [
        WorkflowRoutingPolicy::hosted_only(),
        policy(ExecutionMode::Both),
    ] {
        let routed = route_native_preview_workflow_yaml(PREVIEW_PATH, workflow(false), &policy)
            .expect("native preview source-bound workflow accepted");
        let text = render_yaml(&routed);
        assert!(text.contains(&format!("runs-on: {PAYLOAD_RUNNER}\n")));
        assert!(text.contains(&format!("matrix: {PAYLOAD_MATRIX}\n")));
        assert!(text.contains(&format!("runs-on: {ASSEMBLE_RUNNER}\n")));
        let parsed = saphyr::Yaml::load_from_str(&text).expect("parse routed workflow");
        parsed::validate_workflow(PREVIEW_PATH, &parsed[0], &policy)
            .expect("final hosted native contract accepted");
    }
}

#[test]
fn scale_set_routes_source_and_native_preview_runner_expressions_without_a_container() {
    let policy = policy(ExecutionMode::ScaleSet);
    let routed = route_native_preview_workflow_yaml(PREVIEW_PATH, workflow(true), &policy)
        .expect("native preview route accepted");
    let text = render_yaml(&routed);
    let selector_text = format!("[{}, {}]", selector().labels()[0], selector().labels()[1]);
    assert_eq!(
        text.matches(&format!("runs-on: {selector_text}\n")).count(),
        3
    );
    assert!(!text.contains("container:"));
    let parsed = saphyr::Yaml::load_from_str(&text).expect("parse routed workflow");
    parsed::validate_workflow(PREVIEW_PATH, &parsed[0], &policy)
        .expect("Scale Set output retains the native source and matrix contract");
}

#[test]
fn native_preview_expression_allowlist_rejects_mutated_source_matrix_needs_and_operation() {
    let hosted = WorkflowRoutingPolicy::hosted_only();
    let mut document = workflow(false);
    *field_mut(&mut document, PAYLOAD_JOB, "runs-on") = Yaml::str("${{ matrix.untrusted }}");
    assert!(validate_emitted_workflow(PREVIEW_PATH, &document, &hosted).is_err());

    let mut document = workflow(false);
    *field_mut(&mut document, PAYLOAD_JOB, "needs") = Yaml::Seq(vec![Yaml::str("another-job")]);
    assert!(validate_emitted_workflow(PREVIEW_PATH, &document, &hosted).is_err());

    let mut document = workflow(false);
    let strategy = field_mut(&mut document, PAYLOAD_JOB, "strategy");
    let Yaml::Map(fields) = strategy else {
        panic!("strategy map")
    };
    let Some((_, matrix)) = fields.iter_mut().find(|(name, _)| name == "matrix") else {
        panic!("matrix field")
    };
    *matrix = Yaml::str("${{ fromJSON(needs.other.outputs.payload_matrix) }}");
    assert!(validate_emitted_workflow(PREVIEW_PATH, &document, &hosted).is_err());

    let mut document = workflow(false);
    let source = field_mut(&mut document, SOURCE_JOB, "steps");
    let Yaml::Seq(steps) = source else {
        panic!("steps sequence")
    };
    let Yaml::Map(fields) = &mut steps[0] else {
        panic!("source step map")
    };
    let Some((_, Yaml::Map(env))) = fields.iter_mut().find(|(name, _)| name == "env") else {
        panic!("source env")
    };
    let Some((_, operation)) = env
        .iter_mut()
        .find(|(name, _)| name == "VELNOR_INTERNAL_OP")
    else {
        panic!("operation")
    };
    *operation = Yaml::str("execute-release-producer-v1");
    assert!(validate_emitted_workflow(PREVIEW_PATH, &document, &hosted).is_err());
}

#[test]
fn scale_set_contract_requires_source_runner_admission_flag_and_hosted_rejects_it() {
    let scale_set = policy(ExecutionMode::ScaleSet);
    assert!(validate_emitted_workflow(PREVIEW_PATH, &workflow(false), &scale_set).is_err());
    let hosted = WorkflowRoutingPolicy::hosted_only();
    assert!(validate_emitted_workflow(PREVIEW_PATH, &workflow(true), &hosted).is_err());
}
