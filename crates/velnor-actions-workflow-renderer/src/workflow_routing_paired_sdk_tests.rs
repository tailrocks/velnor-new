use velnor_actions_contract::{ExecutionMode, ScaleSetSelector};

use super::QualificationSource;
use crate::workflow_routing::{WorkflowRoutingPolicy, validate_generated_workflow_calls};
use crate::{RenderedFile, yaml::Yaml};

fn selector() -> ScaleSetSelector {
    ScaleSetSelector::try_new(
        "linux-amd64",
        &["velnor".to_owned(), "linux-amd64".to_owned()],
    )
    .expect("valid test selector")
}

#[test]
fn source_factory_rejects_changed_modes_needs_or_selector() {
    let selector = selector();
    let mut builder = source_builder();
    let consumer = source_consumer(&selector);
    set_field(&mut builder.1, "if", Yaml::str("inputs.mode == 'both'"));
    assert!(
        QualificationSource::from_generated_jobs(
            builder,
            consumer,
            "ubuntu-26.04".to_owned(),
            &selector,
        )
        .is_err()
    );

    let builder = source_builder();
    let mut consumer = source_consumer(&selector);
    set_field(
        &mut consumer.1,
        "needs",
        Yaml::Seq(vec![Yaml::str("other-job")]),
    );
    assert!(
        QualificationSource::from_generated_jobs(
            builder,
            consumer,
            "ubuntu-26.04".to_owned(),
            &selector,
        )
        .is_err()
    );

    let builder = source_builder();
    let mut consumer = source_consumer(&selector);
    set_field(
        &mut consumer.1,
        "runs-on",
        Yaml::Flow(vec!["velnor".to_owned(), "other-scale-set".to_owned()]),
    );
    assert!(
        QualificationSource::from_generated_jobs(
            builder,
            consumer,
            "ubuntu-26.04".to_owned(),
            &selector,
        )
        .is_err()
    );
}

#[test]
fn no_bound_source_does_not_exempt_containerless_scale_set_jobs() {
    let selector = selector();
    let policy = WorkflowRoutingPolicy::configured(ExecutionMode::ScaleSet, selector, None);
    let file = RenderedFile {
        path: ".github/workflows/qualification.yml".to_owned(),
        bytes: "name: Qualification\non: push\njobs:\n  paired-sdk-tls:\n    if: inputs.mode == 'paired-sdk-tls'\n    needs: [paired-sdk-build]\n    runs-on: [velnor, linux-amd64]\n    steps: []\n".to_owned(),
    };

    assert!(validate_generated_workflow_calls(&[file], &policy).is_err());
}

fn source_builder() -> (String, Yaml) {
    (
        "paired-sdk-build".to_owned(),
        Yaml::Map(vec![
            (
                "if".to_owned(),
                Yaml::str("inputs.mode == 'paired-sdk-build' || inputs.mode == 'paired-sdk-tls'"),
            ),
            ("runs-on".to_owned(), Yaml::str("ubuntu-26.04")),
            ("steps".to_owned(), Yaml::Seq(Vec::new())),
        ]),
    )
}

fn source_consumer(selector: &ScaleSetSelector) -> (String, Yaml) {
    (
        "paired-sdk-tls".to_owned(),
        Yaml::Map(vec![
            (
                "if".to_owned(),
                Yaml::str("inputs.mode == 'paired-sdk-tls'"),
            ),
            ("runs-on".to_owned(), Yaml::Flow(selector.labels().to_vec())),
            (
                "needs".to_owned(),
                Yaml::Seq(vec![Yaml::str("paired-sdk-build")]),
            ),
            ("steps".to_owned(), Yaml::Seq(Vec::new())),
        ]),
    )
}

fn set_field(job: &mut Yaml, name: &str, replacement: Yaml) {
    let Yaml::Map(fields) = job else {
        panic!("job fixture must be a map");
    };
    let (_, value) = fields
        .iter_mut()
        .find(|(key, _)| key == name)
        .expect("field exists");
    *value = replacement;
}
