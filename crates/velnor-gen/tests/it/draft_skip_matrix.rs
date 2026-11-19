// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Pins the rendered workflow for every draft-skip configuration, so a refactor
//! of how draft pull requests are skipped cannot change what CI runs without
//! showing up here. Each pin is an FNV-1a hash of the whole rendered workflow;
//! a mismatch prints the full workflow text.

use crate::facts::TestResult;
use crate::pr_ci::render;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(FNV_OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME)
    })
}

fn pinned(config: &str, expected: u64) -> TestResult {
    let text = render("0.1.86", config)?;
    let actual = fnv1a(text.as_bytes());
    assert_eq!(
        actual, expected,
        "rendered workflow changed (actual hash {actual:#018x}):\n{text}"
    );
    Ok(())
}

#[test]
fn draft_skip_off_with_pr_ci_always_renders_the_pinned_workflow() -> TestResult {
    pinned("shape = \"lint-test\"\n", PIN_OFF_ALWAYS)
}

#[test]
fn draft_skip_on_with_pr_ci_always_renders_the_pinned_workflow() -> TestResult {
    pinned(
        "shape = \"lint-test\"\nskip_draft_pull_requests = true\n",
        PIN_ON_ALWAYS,
    )
}

#[test]
fn draft_skip_off_with_pr_ci_label_renders_the_pinned_workflow() -> TestResult {
    pinned("pr_ci = \"label\"\nshape = \"lint-test\"\n", PIN_OFF_LABEL)
}

#[test]
fn draft_skip_on_with_pr_ci_label_is_refused() {
    let refused = render(
        "0.1.86",
        "pr_ci = \"label\"\nshape = \"lint-test\"\nskip_draft_pull_requests = true\n",
    );
    assert!(
        refused.is_err(),
        "pr_ci = label with skip_draft must be refused"
    );
}

const PIN_OFF_ALWAYS: u64 = 0x9be8_0ee1_8d1d_4475;
const PIN_ON_ALWAYS: u64 = 0xdad0_71ff_2d3b_692d;
const PIN_OFF_LABEL: u64 = 0xc9e8_6da7_8ed0_7f2a;
