// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{RustSuiteConfig, validate_rust_suites};

#[test]
fn a_suite_cannot_take_the_state_seal_name() {
    let mut suite = RustSuiteConfig::new("project-check", "default");
    suite.workflow = Some("state-seal".to_owned());
    let error = validate_rust_suites(&[suite], ".velnor/config.toml")
        .expect_err("a suite must not claim the state-seal workflow");
    assert!(
        error
            .to_string()
            .contains("reserved_suite_workflow:state-seal"),
        "{error}"
    );
}
