// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{PolicyTools, RepoPath};
use velnor_rust::policy_runs;

use crate::facts::TestResult;

#[test]
fn a_cargo_deny_exceptions_file_in_a_selected_root_runs_deny_only() -> TestResult {
    let both = PolicyTools {
        deny: true,
        machete: true,
    };
    for name in [
        "crates/a/deny.exceptions.toml",
        "crates/a/.deny.exceptions.toml",
        "crates/a/.cargo/deny.exceptions.toml",
    ] {
        let changed = BTreeSet::from([RepoPath::new(name)?]);
        let runs = policy_runs(both, &changed, false, true);
        assert_eq!((runs.deny, runs.machete), (true, false), "{name}");
    }
    Ok(())
}
