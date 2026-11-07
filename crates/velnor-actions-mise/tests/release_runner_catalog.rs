// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_mise::catalog::{qualified_cargo_semver_checks, qualified_release_plz};

#[test]
fn rust_publisher_tools_require_the_supported_hosted_runner() {
    assert!(qualified_cargo_semver_checks("ubuntu-26.04").is_some());
    assert!(qualified_release_plz("0.4.3", "ubuntu-26.04").is_some());
    for runner in ["ubuntu-24.04", "ubuntu-latest", "ubuntu-26.04-arm"] {
        assert!(qualified_cargo_semver_checks(runner).is_none());
    }
}
