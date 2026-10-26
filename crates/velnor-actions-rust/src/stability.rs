// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::evidence::is_generated_output;
use crate::profile::{CompileDriver, TestRunner};
use crate::scan::{has_adjacent, has_command, strip_comment};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommittedProfile {
    pub compile_driver: CompileDriver,
    pub test_runner: TestRunner,
}

#[must_use]
pub fn read_committed_profile_for_comparison(content: &str) -> Option<CommittedProfile> {
    if !is_generated_output(content) {
        return None;
    }
    let mut mbx = false;
    let mut nextest = false;
    let mut cargo_test = false;
    for line in content.lines().skip(1) {
        let code = strip_comment(line);
        if code.trim().is_empty() {
            continue;
        }
        mbx |= invokes_mbx(code);
        nextest |= invokes_nextest(code);
        cargo_test |= invokes_cargo_test(code);
    }
    if !(mbx || nextest || cargo_test) {
        return None;
    }
    Some(CommittedProfile {
        compile_driver: if mbx {
            CompileDriver::Mbx
        } else {
            CompileDriver::Cargo
        },
        test_runner: if nextest {
            TestRunner::CargoNextest
        } else {
            TestRunner::CargoTest
        },
    })
}

#[must_use]
pub fn committed_profile_differs(
    committed: Option<&CommittedProfile>,
    driver: CompileDriver,
    runner: TestRunner,
) -> bool {
    committed
        .is_some_and(|previous| previous.compile_driver != driver || previous.test_runner != runner)
}

fn invokes_mbx(code: &str) -> bool {
    has_command(code, "mbx")
        || has_command(code, "mr-boxington")
        || has_command(code, "mr_boxington")
}

fn invokes_nextest(code: &str) -> bool {
    has_command(code, "nextest") || has_command(code, "cargo-nextest")
}

fn invokes_cargo_test(code: &str) -> bool {
    has_adjacent(code, "cargo", "test") || has_adjacent(code, "mbx", "test")
}
