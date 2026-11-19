// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Repository pin and freshness policy checks, driven by the
//! `VELNOR_REPO_POLICY_*` environment (see `scripts/check-freshness.sh`).

use std::process::ExitCode;

fn main() -> ExitCode {
    match velnor_freshness::request::gate_root() {
        Some(root) => velnor_freshness::request::run(&root, "velnor-freshness"),
        None => {
            eprintln!(
                "velnor-freshness: set VELNOR_REPO_POLICY_ACTION and VELNOR_REPO_POLICY_ROOT \
                 (an existing directory)"
            );
            ExitCode::from(2)
        }
    }
}
