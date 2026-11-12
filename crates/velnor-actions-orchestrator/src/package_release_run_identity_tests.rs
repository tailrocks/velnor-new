// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{WorkflowMetadata, admit_workflow, validate_workflow_metadata};

const PATH: &str = ".github/workflows/binary-preview.yml";
const NAME: &str = "Rust binary preview";

fn workflow(state: &str) -> WorkflowMetadata {
    WorkflowMetadata {
        id: 388_393_922,
        name: NAME.to_owned(),
        path: PATH.to_owned(),
        state: state.to_owned(),
    }
}

#[test]
fn a_current_admission_requires_an_active_workflow() {
    assert!(validate_workflow_metadata(&workflow("active"), PATH, NAME).is_ok());
    assert!(validate_workflow_metadata(&workflow("deleted"), PATH, NAME).is_err());
    assert!(validate_workflow_metadata(&workflow("disabled_manually"), PATH, NAME).is_err());
}

#[test]
fn an_unpinned_admission_rejects_a_deleted_workflow_through_the_shared_decision() {
    assert!(admit_workflow(&workflow("active"), PATH, NAME, None).is_ok());
    assert!(admit_workflow(&workflow("deleted"), PATH, NAME, None).is_err());
}
