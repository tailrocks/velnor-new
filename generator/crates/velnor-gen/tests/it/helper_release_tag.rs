// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::HelperVersion;
use velnor_rust::{commit_url, sums_url};

use crate::lock::locked;
use crate::pipeline::workflow;

type TestResult = Result<(), Box<dyn Error>>;

const RELEASES: &str = "https://github.com/tailrocks/velnor-new/releases/download";

#[test]
fn a_helper_below_0_1_85_is_tagged_rust_ci_v_and_0_1_85_and_later_tools_v() -> TestResult {
    for (version, tag) in [
        ("0.1.79", "rust-ci-v0.1.79"),
        ("0.1.84", "rust-ci-v0.1.84"),
        ("0.1.100", "tools-v0.1.100"),
        ("0.1.85", "tools-v0.1.85"),
        ("0.1.86", "tools-v0.1.86"),
        ("0.2.0", "tools-v0.2.0"),
        ("1.0.0", "tools-v1.0.0"),
    ] {
        assert_eq!(HelperVersion::new(version)?.release_tag(), tag, "{version}");
    }
    Ok(())
}

#[test]
fn the_tag_comment_after_the_setup_velnor_pin_names_the_tag_that_exists() -> TestResult {
    for (version, comment) in [
        ("0.1.84", "# rust-ci-v0.1.84"),
        ("0.1.85", "# tools-v0.1.85"),
    ] {
        let text = workflow(&locked(version)?)?;
        let line = text
            .lines()
            .find(|line| line.contains("actions/setup-velnor@"))
            .ok_or("no setup-velnor step")?;
        assert!(line.ends_with(&format!("  {comment}")), "{version}: {line}");
    }
    Ok(())
}

#[test]
fn pin_helper_downloads_the_sums_and_commit_from_the_tag_of_the_version() -> TestResult {
    for (version, tag) in [("0.1.84", "rust-ci-v0.1.84"), ("0.1.85", "tools-v0.1.85")] {
        let version = HelperVersion::new(version)?;
        assert_eq!(sums_url(&version), format!("{RELEASES}/{tag}/SHA256SUMS"));
        assert_eq!(commit_url(&version), format!("{RELEASES}/{tag}/COMMIT"));
    }
    Ok(())
}
