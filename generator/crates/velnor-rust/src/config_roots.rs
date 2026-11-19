// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{DenyConfig, RepoPath, RustCiConfig, RustRoot, TestLane, TestSetup};

use crate::config_deny::RawDeny;
use crate::config_test_lane::RawTestLane;
use crate::config_test_setup::RawTestSetup;
use crate::error::{DetectError, Reason};

pub(crate) fn parse(
    setup: Vec<RawTestSetup>,
    lanes: Vec<RawTestLane>,
    deny: Option<RawDeny>,
) -> Result<(Vec<TestSetup>, Vec<TestLane>, DenyConfig), Reason> {
    Ok((
        crate::config_test_setup::parse(setup)?,
        crate::config_test_lane::parse(lanes)?,
        crate::config_deny::parse(deny)?,
    ))
}

pub(crate) fn check(
    config: &RustCiConfig,
    rust_roots: &[RustRoot],
    tracked: &BTreeSet<RepoPath>,
) -> Result<(), DetectError> {
    crate::config_test_setup::check(config, rust_roots)?;
    crate::config_test_lane::check(config, rust_roots)?;
    crate::config_deny::check(config, rust_roots, tracked)
}
