// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_rust::{CONFIG_PATH, DetectError};

use crate::facts::TestResult;
use crate::pipeline::view;

#[test]
fn the_default_branch_is_main_unless_the_config_names_another_valid_branch() -> TestResult {
    let facts = |config: &[u8]| -> Result<_, Box<dyn std::error::Error>> {
        Ok(velnor_rust::detect(&view(&["Cargo.toml"], Some(config))?))
    };
    assert_eq!(facts(b"")??.config.default_branch, "main");
    assert_eq!(
        facts(b"default_branch = \"release/2.x\"\n")??
            .config
            .default_branch,
        "release/2.x"
    );
    for bad in [
        "",
        "a b",
        "a..b",
        "-x",
        "x/",
        "a~b",
        "a:b",
        "a//b",
        "x.",
        "a@{b",
        ".hidden",
        "foo/.bar",
        "foo.lock",
        "foo.lock/bar",
        "HEAD",
    ] {
        let result = facts(format!("default_branch = \"{bad}\"\n").as_bytes())?;
        assert!(
            matches!(&result, Err(DetectError::Invalid { path, .. }) if *path == CONFIG_PATH),
            "{bad:?} gave {result:?}"
        );
    }
    Ok(())
}
