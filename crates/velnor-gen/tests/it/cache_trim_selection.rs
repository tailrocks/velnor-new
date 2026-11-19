// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{AllTrigger, RustSelection};

use crate::select::{all, assert_selects, two_roots};

#[test]
fn the_generated_cache_cleanup_workflow_selects_nothing_while_the_main_workflow_selects_everything()
{
    let facts = two_roots();
    assert_selects(
        &facts,
        vec![
            (
                &[".github/workflows/rust-cache-trim.yml"],
                vec![RustSelection::None, RustSelection::None],
            ),
            (
                &[".github/workflows/rust-ci.yml"],
                vec![
                    all(AllTrigger::GeneratedWorkflow),
                    all(AllTrigger::GeneratedWorkflow),
                ],
            ),
        ],
    );
}
