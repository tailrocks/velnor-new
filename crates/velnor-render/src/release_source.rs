// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::{FamilyTrigger, ReleasePlan};

use crate::release::{FROM_A_PASSED_PUSH, VERIFIED_COMMIT};

const PUSHED_TAG: &str = "github.ref_type == 'tag'";
const PUSHED_COMMIT: &str = "${{ github.sha }}";

#[derive(Clone, Copy)]
pub(crate) struct Source {
    pub(crate) verified: &'static str,
    pub(crate) passed: &'static str,
    pub(crate) tag_push: bool,
}

pub(crate) fn source_of(plan: &ReleasePlan) -> Source {
    let tag_push = plan
        .family
        .as_ref()
        .is_some_and(|family| family.trigger == FamilyTrigger::TagPush);
    if tag_push {
        Source {
            verified: PUSHED_COMMIT,
            passed: PUSHED_TAG,
            tag_push,
        }
    } else {
        Source {
            verified: VERIFIED_COMMIT,
            passed: FROM_A_PASSED_PUSH,
            tag_push,
        }
    }
}
