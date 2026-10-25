use crate::errors::ContractError;

use super::{BuildTask, NativeImageTask, VerificationTask};

pub const WORKFLOW_TASK_JOB_PREFIX: &str = "task-";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum WorkflowTask {
    Verification(VerificationTask),

    Build(BuildTask),

    NativeImage(NativeImageTask),
}

impl WorkflowTask {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Verification(task) => &task.id,
            Self::Build(task) => &task.id,
            Self::NativeImage(task) => &task.id,
        }
    }

    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        match self {
            Self::Verification(task) => task.validate(file),
            Self::Build(task) => task.validate(file),
            Self::NativeImage(task) => task.validate(file),
        }
    }
}

#[must_use]
pub fn is_valid_workflow_task_id(id: &str) -> bool {
    const RESERVED: [&str; 6] = [
        "jactionlint",
        "candidate",
        "plan",
        "required",
        "task",
        "velnor-task",
    ];
    !id.is_empty()
        && id.len() <= 48
        && !RESERVED.contains(&id)
        && id.as_bytes()[0].is_ascii_lowercase()
        && (id.as_bytes()[id.len() - 1].is_ascii_lowercase()
            || id.as_bytes()[id.len() - 1].is_ascii_digit())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !id.contains("--")
}
