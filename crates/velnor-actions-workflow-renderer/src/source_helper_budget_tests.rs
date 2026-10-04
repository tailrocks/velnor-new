use super::{
    PreparedTransport, RenderError, TransportAdmission, admit_source_helper_transport,
    source_helper_step, source_helper_step_to_yaml,
};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CompiledSourceHelper, HelperInvocation, SourceBoundHelper, SourceBoundOperation, Step, StepKind,
};

fn record(operation: SourceBoundOperation, body: &str, args: Vec<String>) -> CompiledSourceHelper {
    let source = velnor_actions_contract::generated_source("0.1.0", body).expect("source");
    let digest = velnor_actions_contract::compiled_source_sha256(source.as_bytes());
    let descriptor =
        SourceBoundHelper::compiled(operation, operation.path(), &digest).expect("descriptor");
    let invocation = HelperInvocation::compiled(descriptor, args, Vec::new()).expect("invocation");
    CompiledSourceHelper::compiled(invocation, source).expect("record")
}

fn rendered_parts(document: crate::Yaml) -> (String, BTreeMap<String, String>) {
    let crate::Yaml::Map(entries) = document else {
        panic!("helper yaml map")
    };
    let run = entries
        .iter()
        .find(|(key, _)| key == "run")
        .and_then(|(_, value)| match value {
            crate::Yaml::Str(value) => Some(value.clone()),
            _ => None,
        })
        .expect("helper run");
    let environment = entries
        .iter()
        .find(|(key, _)| key == "env")
        .and_then(|(_, value)| match value {
            crate::Yaml::Map(values) => Some(
                values
                    .iter()
                    .filter_map(|(key, value)| match value {
                        crate::Yaml::Str(value) => Some((key.clone(), value.clone())),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
        .expect("helper environment");
    (run, environment)
}

#[test]
fn preflight_output_is_exactly_the_rendered_transport() {
    let record = record(
        SourceBoundOperation::RustPrepareRootLinux,
        "exit 0\n",
        vec!["tool".to_owned()],
    );
    let prepared = match admit_source_helper_transport(&record, "ubuntu-26.04").expect("preflight")
    {
        TransportAdmission::Supported(prepared) => prepared,
        TransportAdmission::UnsupportedBudget(_) => panic!("small helper budget"),
    };
    let step = source_helper_step("Prepare", &record, BTreeMap::new()).expect("step");
    let rendered = source_helper_step_to_yaml(
        &step,
        std::slice::from_ref(&record),
        "0.1.0",
        "ubuntu-26.04",
    )
    .expect("yaml");
    let (run, environment) = rendered_parts(rendered);
    let PreparedTransport {
        run: prepared_run,
        environment: prepared_environment,
    } = prepared;
    assert_eq!(run, prepared_run);
    assert_eq!(environment, prepared_environment);
}

#[test]
fn preflight_reports_measured_macos_budget_without_allocating_yaml() {
    let marker = velnor_actions_contract::generated_source("0.1.0", "").expect("marker");
    let body = "x".repeat(262_144 - marker.len());
    let args = vec!["x".repeat(65_536); 8];
    let record = record(SourceBoundOperation::RustPrepareRootLinux, &body, args);
    let admission = admit_source_helper_transport(&record, "macos-26").expect("budget outcome");
    let TransportAdmission::UnsupportedBudget(budget) = admission else {
        panic!("macOS budget must be measured");
    };
    assert_eq!(budget.reason, "unsupported_environment_limit");
    assert!(budget.measured >= budget.limit);
    assert_eq!(budget.limit, 1_048_576);
}

#[test]
fn raw_oversize_is_typed_and_never_becomes_prepared_transport() {
    let record = record(
        SourceBoundOperation::RustPrepareRootLinux,
        "exit 0\n",
        vec!["x".repeat(4 * 1024 * 1024)],
    );
    let admission = admit_source_helper_transport(&record, "ubuntu-26.04").expect("budget result");
    let TransportAdmission::UnsupportedBudget(budget) = admission else {
        panic!("raw oversize must not prepare transport");
    };
    assert_eq!(budget.reason, "raw_argument_size");
    assert_eq!(budget.measured, 4 * 1024 * 1024);
    assert_eq!(
        budget.limit,
        velnor_actions_contract::SOURCE_HELPER_ARGUMENT_BYTES_MAX
    );

    assert!(source_helper_step("Oversized", &record, BTreeMap::new()).is_err());
    let step = Step {
        id: None,
        name: "Oversized".to_owned(),
        condition: None,
        kind: StepKind::SourceBoundHelper {
            invocation: record.invocation().clone(),
            env: BTreeMap::new(),
        },
    };
    assert!(matches!(
        source_helper_step_to_yaml(
            &step,
            std::slice::from_ref(&record),
            "0.1.0",
            "ubuntu-26.04"
        ),
        Err(RenderError::UnsupportedHelperTransport(_))
    ));
}

#[test]
fn raw_oversize_still_rejects_wrong_runner_and_environment() {
    let record = record(
        SourceBoundOperation::RustPrepareRootLinux,
        "exit 0\n",
        vec!["x".repeat(4 * 1024 * 1024)],
    );
    let unknown = admit_source_helper_transport(&record, "windows-2025").expect_err("runner");
    assert!(unknown.to_string().contains("unsupported_runner"));
    let forged = record.with_environment(BTreeMap::from([(
        "GH_TOKEN".to_owned(),
        "forged".to_owned(),
    )]));
    assert!(admit_source_helper_transport(&forged, "ubuntu-26.04").is_err());
}

#[test]
fn argument_count_and_controls_remain_constructor_hard_errors() {
    let base = record(
        SourceBoundOperation::RustPrepareRootLinux,
        "exit 0\n",
        Vec::new(),
    );
    let descriptor = base.invocation().descriptor().clone();
    assert!(
        HelperInvocation::compiled(descriptor.clone(), vec!["x".to_owned(); 2049], Vec::new())
            .is_err()
    );
    assert!(
        HelperInvocation::compiled(descriptor, vec!["bad\nvalue".to_owned()], Vec::new()).is_err()
    );
}

#[test]
fn preflight_rejects_unknown_runner_and_forged_authority() {
    let record = record(
        SourceBoundOperation::RustPrepareRootLinux,
        "exit 0\n",
        Vec::new(),
    );
    let unknown = admit_source_helper_transport(&record, "windows-2025").expect_err("runner");
    assert!(unknown.to_string().contains("unsupported_runner"));

    let source =
        velnor_actions_contract::generated_source("0.1.0", "tampered\n").expect("forged source");
    let descriptor = SourceBoundHelper::compiled(
        SourceBoundOperation::RustPrepareRootLinux,
        SourceBoundOperation::RustPrepareRootLinux.path(),
        &"0".repeat(64),
    )
    .expect("forged descriptor shape");
    let invocation = HelperInvocation::compiled(descriptor, Vec::new(), Vec::new())
        .expect("forged invocation shape");
    assert!(CompiledSourceHelper::compiled(invocation, source).is_err());

    let forged_environment = record.with_environment(BTreeMap::from([(
        "GH_TOKEN".to_owned(),
        "forged".to_owned(),
    )]));
    assert!(admit_source_helper_transport(&forged_environment, "ubuntu-26.04").is_err());
}

#[test]
fn contract_argument_budget_is_the_raw_512_kibibyte_limit() {
    assert_eq!(
        velnor_actions_contract::SOURCE_HELPER_ARGUMENT_BYTES_MAX,
        512 * 1024
    );
}
