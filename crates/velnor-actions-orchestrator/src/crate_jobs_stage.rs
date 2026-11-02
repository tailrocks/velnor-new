// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{ProposedTask, Stack};
use velnor_actions_rust::tool_needs;

pub(crate) fn needs(task: &ProposedTask) -> velnor_actions_rust::ToolNeeds {
    if Stack::from_id(&task.stack_id) == Some(Stack::Tofu) {
        return velnor_actions_rust::ToolNeeds {
            mbx: false,
            nextest: false,
        };
    }
    tool_needs(&task.identity.compile_driver, &task.identity.test_runner)
}

pub(crate) fn is_rust(task: &ProposedTask) -> bool {
    !is_opentofu(task)
}

pub(crate) fn is_mbx(task: &ProposedTask) -> bool {
    needs(task).mbx
}

pub(crate) fn is_nextest(task: &ProposedTask) -> bool {
    needs(task).nextest
}

pub(crate) fn is_opentofu(task: &ProposedTask) -> bool {
    Stack::from_id(&task.stack_id) == Some(Stack::Tofu)
}
