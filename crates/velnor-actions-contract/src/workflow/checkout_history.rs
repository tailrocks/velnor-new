// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use super::plan::{WorkflowEvent, selects_affected_work};

const _: () = assert!(
    !selects_affected_work(WorkflowEvent::Push),
    "a push must not select by diff, so a shallow push checkout is only sound while push never reads history"
);

const FULL_FETCH_DEPTH: &str = "0";
const SHALLOW_PUSH_BRANCH: &str = "main";
const SHALLOW_ON_PUSH_FETCH_DEPTH: &str = "${{ github.event_name == 'push' && '1' || '0' }}";
const MERGE_PARENTS_FETCH_DEPTH: &str = "${{ github.event_name == 'push' && '1' || '2' }}";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckoutHistory {
    #[default]
    Full,
    ShallowOnPush,
    #[serde(skip_deserializing)]
    ShallowWithMergeParents,
}

impl CheckoutHistory {
    #[must_use]
    pub const fn fetch_depth_input(self) -> &'static str {
        match self {
            Self::Full => FULL_FETCH_DEPTH,
            Self::ShallowOnPush => SHALLOW_ON_PUSH_FETCH_DEPTH,
            Self::ShallowWithMergeParents => MERGE_PARENTS_FETCH_DEPTH,
        }
    }

    #[must_use]
    pub fn parse_fetch_depth(value: &str) -> Option<Self> {
        match value {
            FULL_FETCH_DEPTH => Some(Self::Full),
            SHALLOW_ON_PUSH_FETCH_DEPTH => Some(Self::ShallowOnPush),
            MERGE_PARENTS_FETCH_DEPTH => Some(Self::ShallowWithMergeParents),
            _ => None,
        }
    }

    #[must_use]
    pub const fn is_full(&self) -> bool {
        matches!(self, Self::Full)
    }

    #[must_use]
    pub fn permits_push_branches(self, push_branches: &[String]) -> bool {
        match self {
            Self::Full => true,
            Self::ShallowOnPush | Self::ShallowWithMergeParents => {
                push_branches == [SHALLOW_PUSH_BRANCH]
            }
        }
    }
}
