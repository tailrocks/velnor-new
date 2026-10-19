//! Shared fixture construction for attach tests.

use super::*;

/// Bare IR shell shared by the attach fixtures.
pub(super) fn bare_ir(jobs: BTreeMap<String, velnor_actions_contract::Job>) -> WorkflowIr {
    WorkflowIr {
        name: "CI".to_owned(),
        triggers: Trigger {
            pull_request_types: Vec::new(),
            push_branches: Vec::new(),
            merge_group: false,
            workflow_dispatch: None,
            workflow_call: false,
            schedule: None,
        },
        permissions: Permissions::default(),
        concurrency: Concurrency {
            group: "g".to_owned(),
            cancel_in_progress: "c".to_owned(),
        },
        jobs,
        cache_mode: None,
    }
}
