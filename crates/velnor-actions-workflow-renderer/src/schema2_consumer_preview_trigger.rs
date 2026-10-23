//! Dedicated native preview completion admission; generic triggers stay restricted.
use crate::{RenderError, yaml::Yaml};
use velnor_actions_contract::{WORKFLOW_DISPLAY_NAME, package_release::NativePreviewEvent};

pub(in super::super) fn trigger(branch: &str) -> (String, Yaml) {
    (
        NativePreviewEvent::WorkflowRun.as_str().to_owned(),
        Yaml::Map(vec![
            (
                "workflows".to_owned(),
                Yaml::Seq(vec![Yaml::annotated(
                    WORKFLOW_DISPLAY_NAME,
                    "zizmor: ignore[dangerous-triggers]",
                )]),
            ),
            ("types".to_owned(), Yaml::Seq(vec![Yaml::str("completed")])),
            ("branches".to_owned(), Yaml::Seq(vec![Yaml::str(branch)])),
        ]),
    )
}

/// The annotation covers zizmor's whole `on` span, so admit only this closed shape.
/// Unknown/duplicate/sibling triggers must fail before any annotated bytes emit.
pub(in super::super) fn validate(document: &Yaml, recovery: Yaml) -> Result<(), RenderError> {
    let Yaml::Map(fields) = document else {
        return Err(trigger_error());
    };
    let mut triggers = fields.iter().filter(|(key, _)| key == "on");
    let expected = Yaml::Map(vec![
        trigger("main"),
        ("workflow_dispatch".to_owned(), recovery),
    ]);
    match (triggers.next(), triggers.next()) {
        (Some((_, actual)), None) if actual == &expected => Ok(()),
        _ => Err(trigger_error()),
    }
}

fn trigger_error() -> RenderError {
    RenderError::InvalidWorkflow("native_preview_trigger_mismatch".to_owned())
}
