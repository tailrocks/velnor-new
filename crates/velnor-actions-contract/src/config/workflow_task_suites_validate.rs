//! Validation for the workflow task-suite inventory.

use super::{ContractError, WorkflowConfig};

pub(super) fn validate(workflow: &WorkflowConfig, file: &str) -> Result<(), ContractError> {
    let mut previous = None;
    for (index, suite) in workflow.task_suites.iter().enumerate() {
        let prefix = format!("workflow.task_suites[{index}]");
        suite.validate(file, &prefix)?;
        if previous.is_some_and(|previous_id: &str| previous_id >= suite.id.as_str()) {
            let problem = if previous == Some(suite.id.as_str()) {
                format!("duplicate_task_suite_id:{}", suite.id)
            } else {
                "task_suites_must_be_sorted_by_id".to_owned()
            };
            return Err(ContractError::config(file, "workflow.task_suites", problem));
        }
        previous = Some(suite.id.as_str());
        let path = format!(".github/workflows/{}.yml", suite.workflow);
        if workflow.preserve_workflows.contains(&path) {
            return Err(ContractError::config(
                file,
                "workflow.preserve_workflows",
                format!("preserve_workflow_generator_owned:{path}"),
            ));
        }
    }
    for (index, suite) in workflow.task_suites.iter().enumerate() {
        if let Some(snapshot) = &suite.evidence_snapshot {
            snapshot.validate_inventory(
                &workflow.task_suites,
                &suite.id,
                file,
                &format!("workflow.task_suites[{index}].evidence_snapshot"),
            )?;
        }
    }
    Ok(())
}
