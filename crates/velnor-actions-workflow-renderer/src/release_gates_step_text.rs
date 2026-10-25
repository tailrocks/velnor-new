use crate::RenderError;
use velnor_actions_contract::{Step, StepKind};

pub(super) fn payload_texts(step: &Step) -> Vec<&str> {
    match &step.kind {
        StepKind::Action { uses, with, env } => std::iter::once(uses.as_str())
            .chain(with.keys().map(String::as_str))
            .chain(with.values().map(String::as_str))
            .chain(env.keys().map(String::as_str))
            .chain(env.values().map(String::as_str))
            .collect(),
        StepKind::Shell { run, env } => run
            .iter()
            .map(String::as_str)
            .chain(env.keys().map(String::as_str))
            .chain(env.values().map(String::as_str))
            .collect(),
        StepKind::Internal { .. } => Vec::new(),
        StepKind::NativeCheckExecution {
            check_id,
            task_id,
            sdk_version,
            ..
        } => vec![check_id.as_str(), task_id.as_str(), sdk_version.as_str()],
        StepKind::TaskExecution {
            argv,
            env,
            task_id,
            task_digest,
            matrix_id,
            matrix_key,
            report_helper_version,
            ..
        } => argv
            .iter()
            .map(String::as_str)
            .chain(env.keys().map(String::as_str))
            .chain(env.values().map(String::as_str))
            .chain([
                task_id.as_str(),
                task_digest.as_str(),
                matrix_id.as_str(),
                matrix_key.as_str(),
                report_helper_version.as_str(),
            ])
            .collect(),
    }
}

pub(super) fn forbid_substring(
    id: &str,
    steps: &[Step],
    needle: &str,
    code: &str,
) -> Result<(), RenderError> {
    for step in steps {
        if payload_texts(step).iter().any(|text| text.contains(needle)) {
            return Err(RenderError::InvalidWorkflow(format!("{code}:{id}")));
        }
    }
    Ok(())
}
