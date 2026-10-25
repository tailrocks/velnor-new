use crate::config::{
    BuildTaskRunner, VelnorConfig, VerificationRunner, WORKFLOW_TASK_JOB_PREFIX, WorkflowTask,
};

pub(super) fn declared_hosted_macos_task_runner(
    config: &VelnorConfig,
    job_id: &str,
) -> Option<&'static str> {
    let task_id = job_id.strip_prefix(WORKFLOW_TASK_JOB_PREFIX)?;
    config.workflow.tasks.iter().find_map(|task| {
        if task.id() != task_id {
            return None;
        }
        let runner = match task {
            WorkflowTask::Build(task) => match task.runner {
                BuildTaskRunner::Macos26Arm64 | BuildTaskRunner::Xcode27Arm64 => {
                    task.runner.runs_on()
                }
            },
            WorkflowTask::Verification(task) => match task.runner {
                VerificationRunner::MacosArm64
                | VerificationRunner::Macos26Arm64
                | VerificationRunner::Xcode27Arm64 => task.runner.runs_on(),
                VerificationRunner::LinuxX64 => return None,
            },
            WorkflowTask::NativeImage(_) => return None,
        };
        Some(runner)
    })
}
