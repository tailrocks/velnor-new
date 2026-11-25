// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_gen::io::recording_of;

use super::{GONE, timed_view};
use crate::facts::TestResult;

#[test]
fn recording_a_sample_reports_a_stale_timings_entry_to_its_output() -> TestResult {
    let mut out = Vec::new();
    let view = timed_view(GONE, "")?;
    recording_of(
        std::path::Path::new("/checkout"),
        view,
        BTreeMap::new(),
        &mut out,
    )?;
    let text = String::from_utf8(out)?;
    assert!(text.contains("gone/Cargo.toml"), "{text}");
    Ok(())
}
