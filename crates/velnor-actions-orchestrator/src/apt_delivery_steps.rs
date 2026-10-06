//! Typed fixed steps and jobs for the APT delivery workflow.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::{JobOutput, StepOutputRef};
use velnor_actions_contract::{ActionOutput, Job, JobTimeout, Step, StepId, StepKind};
use velnor_actions_workflow_renderer::RenderError;

const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";
const UPLOAD: &str = "actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a";

/// Build one SHA-pinned action step.
pub(super) fn action(name: &str, uses: &str, with: BTreeMap<String, String>) -> Step {
    Step {
        id: None,
        name: name.to_owned(),
        condition: None,
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with,
            env: BTreeMap::new(),
        },
    }
}

/// Read-only checkout never persists its token.
pub(super) fn checkout() -> Step {
    action(
        "Checkout",
        CHECKOUT,
        BTreeMap::from([(String::from("persist-credentials"), String::from("false"))]),
    )
}

/// Preserve hidden verification metadata across the isolated jobs.
pub(super) fn upload(id: &str, name: &str, path: &str) -> Result<Step, RenderError> {
    let id = StepId::new(id).map_err(RenderError::Contract)?;
    Ok(Step {
        id: Some(id),
        name: "Upload feed artifact".to_owned(),
        condition: None,
        kind: StepKind::Action {
            uses: UPLOAD.to_owned(),
            with: BTreeMap::from([
                ("if-no-files-found".to_owned(), "error".to_owned()),
                ("include-hidden-files".to_owned(), "true".to_owned()),
                ("name".to_owned(), name.to_owned()),
                ("path".to_owned(), path.to_owned()),
                ("retention-days".to_owned(), "2".to_owned()),
            ]),
            env: BTreeMap::new(),
        },
    })
}

/// Bind downstream consumers to exact upload outputs.
pub(super) fn artifact_outputs(step: &str) -> Result<Vec<JobOutput>, RenderError> {
    let step_id = StepId::new(step).map_err(RenderError::Contract)?;
    Ok(vec![
        JobOutput {
            name: "artifact_id".to_owned(),
            value: StepOutputRef {
                step_id: step_id.clone(),
                output: ActionOutput::ArtifactId,
            },
        },
        JobOutput {
            name: "artifact_digest".to_owned(),
            value: StepOutputRef {
                step_id,
                output: ActionOutput::ArtifactDigest,
            },
        },
    ])
}

/// Common fixed job properties.
pub(super) fn job(
    name: &str,
    runs_on: &str,
    timeout: i64,
    steps: Vec<Step>,
) -> Result<Job, RenderError> {
    let timeout = u16::try_from(timeout)
        .map_err(|_| RenderError::InvalidWorkflow("apt_job_timeout".to_owned()))?;
    let timeout = JobTimeout::new(timeout).map_err(RenderError::Contract)?;
    Ok(Job {
        cache_mode: None,
        display_name: name.to_owned(),
        runs_on: runs_on.to_owned(),
        timeout_minutes: timeout,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        source_producer: None,
        tool_producer: None,
        mbx_producer: None,
        native_pages_deploy: None,
        native_publish: None,
        outputs: Vec::new(),
        steps,
    })
}
