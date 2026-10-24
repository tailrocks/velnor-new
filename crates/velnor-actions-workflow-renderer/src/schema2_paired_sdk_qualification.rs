use velnor_actions_contract::ScaleSetSelector;

use crate::{RenderError, yaml::Yaml};

use super::{ProductReleasePins, Schema2WorkflowRequest, paired_sdk_steps as steps};
use crate::workflow_routing::paired_sdk::{
    BUILD_CONDITION as PAIRED_SDK_BUILD_CONDITION, BUILD_JOB_ID as PAIRED_SDK_BUILD_JOB_ID,
    QualificationSource, TLS_CONDITION as PAIRED_SDK_TLS_CONDITION,
    TLS_JOB_ID as PAIRED_SDK_TLS_JOB_ID,
};

const JOB_TIMEOUT_MINUTES: i64 = 30;
const UNAVAILABLE_COMMAND: &str = "printf '%s\\n' 'paired SDK qualification requires production pins and a configured Scale Set' >&2; exit 1";

pub(super) fn append(
    workflow: Yaml,
    request: &Schema2WorkflowRequest,
) -> Result<(Yaml, Option<QualificationSource>), RenderError> {
    if request.routing.is_scale_set() {
        return Ok((workflow, None));
    }
    let Yaml::Map(mut document) = workflow else {
        return Err(RenderError::InvalidWorkflow(
            "paired_sdk_workflow_not_map".to_owned(),
        ));
    };
    let jobs = document
        .iter_mut()
        .find_map(|(key, value)| (key == "jobs").then_some(value))
        .and_then(|value| match value {
            Yaml::Map(jobs) => Some(jobs),
            _ => None,
        })
        .ok_or_else(|| {
            RenderError::InvalidWorkflow("paired_sdk_workflow_jobs_missing".to_owned())
        })?;

    let Some(selector) = request.routing.paired_sdk_selector() else {
        jobs.push(unavailable_job(&request.hosted_label));
        return Ok((Yaml::Map(document), None));
    };
    let Some(pins) = request.product_release.as_ref() else {
        jobs.push(unavailable_job(&request.hosted_label));
        return Ok((Yaml::Map(document), None));
    };

    let builder = builder_job(&request.hosted_label, pins)?;
    let consumer = consumer_job(selector);
    jobs.push((PAIRED_SDK_BUILD_JOB_ID.to_owned(), builder.1.clone()));
    jobs.push((PAIRED_SDK_TLS_JOB_ID.to_owned(), consumer.1.clone()));
    let source = QualificationSource::from_generated_jobs(
        builder,
        consumer,
        request.hosted_label.clone(),
        selector,
    )?;
    Ok((Yaml::Map(document), Some(source)))
}

fn builder_job(
    hosted_label: &str,
    pins: &ProductReleasePins,
) -> Result<(String, Yaml), RenderError> {
    Ok((
        PAIRED_SDK_BUILD_JOB_ID.to_owned(),
        Yaml::Map(vec![
            ("name".to_owned(), Yaml::str("Build paired SDK fixture")),
            ("if".to_owned(), Yaml::str(PAIRED_SDK_BUILD_CONDITION)),
            ("runs-on".to_owned(), Yaml::str(hosted_label)),
            ("timeout-minutes".to_owned(), Yaml::Int(JOB_TIMEOUT_MINUTES)),
            ("permissions".to_owned(), steps::builder_permissions()),
            ("outputs".to_owned(), steps::builder_outputs()),
            ("steps".to_owned(), Yaml::Seq(steps::builder_steps(pins)?)),
        ]),
    ))
}

fn consumer_job(selector: &ScaleSetSelector) -> (String, Yaml) {
    (
        PAIRED_SDK_TLS_JOB_ID.to_owned(),
        Yaml::Map(vec![
            ("name".to_owned(), Yaml::str("Verify paired SDK HTTPS")),
            ("if".to_owned(), Yaml::str(PAIRED_SDK_TLS_CONDITION)),
            ("runs-on".to_owned(), Yaml::Flow(selector.labels().to_vec())),
            ("timeout-minutes".to_owned(), Yaml::Int(JOB_TIMEOUT_MINUTES)),
            (
                "needs".to_owned(),
                Yaml::Seq(vec![Yaml::str(PAIRED_SDK_BUILD_JOB_ID)]),
            ),
            ("permissions".to_owned(), steps::consumer_permissions()),
            ("steps".to_owned(), Yaml::Seq(steps::consumer_steps())),
        ]),
    )
}

fn unavailable_job(hosted_label: &str) -> (String, Yaml) {
    (
        "paired-sdk-unavailable".to_owned(),
        Yaml::Map(vec![
            (
                "name".to_owned(),
                Yaml::str("Paired SDK qualification unavailable"),
            ),
            ("if".to_owned(), Yaml::str(PAIRED_SDK_BUILD_CONDITION)),
            ("runs-on".to_owned(), Yaml::str(hosted_label)),
            ("timeout-minutes".to_owned(), Yaml::Int(5)),
            (
                "steps".to_owned(),
                Yaml::Seq(vec![Yaml::Map(vec![
                    ("name".to_owned(), Yaml::str("Fail closed")),
                    ("shell".to_owned(), Yaml::str("bash")),
                    ("run".to_owned(), Yaml::str(UNAVAILABLE_COMMAND)),
                ])]),
            ),
        ]),
    )
}
