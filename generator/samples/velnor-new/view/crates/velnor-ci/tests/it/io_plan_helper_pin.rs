// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::io_plan::{plan_command, plan_repo, selected};

#[test]
fn plan_selects_everything_for_a_helper_pin_change_without_an_unknown_path_warning() {
    let repo = plan_repo("helper-pin");
    repo.write(
        ".velnor/rust-ci.lock",
        include_str!("../../../../generator/samples/helper-lock/view/.velnor/rust-ci.lock"),
    );
    let output = plan_command(&repo)
        .args(["plan", "--explain", "--changed", ".velnor/rust-ci.lock"])
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        selected(&output),
        "Cargo.toml: all:global:.velnor/rust-ci.lock\ntools/Cargo.toml: all:global:.velnor/rust-ci.lock\n"
    );
}
