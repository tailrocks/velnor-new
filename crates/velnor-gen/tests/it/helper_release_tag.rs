// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::HelperVersion;
use velnor_rust::{binary_sums_url, commit_url, sums_url};

use crate::lock::locked;
use crate::pipeline::workflow;

type TestResult = Result<(), Box<dyn Error>>;

const RELEASES: &str = "https://github.com/tailrocks/velnor-new/releases/download";

#[test]
fn a_helper_below_0_1_85_is_tagged_rust_ci_v_and_0_1_85_and_later_tools_v() -> TestResult {
    for (version, tag) in [
        ("0.1.81", "rust-ci-v0.1.81"),
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
fn pin_helper_downloads_every_digest_file_from_the_tag_of_the_version() -> TestResult {
    for (version, tag) in [("0.1.84", "rust-ci-v0.1.84"), ("0.1.85", "tools-v0.1.85")] {
        let version = HelperVersion::new(version)?;
        assert_eq!(sums_url(&version), format!("{RELEASES}/{tag}/SHA256SUMS"));
        assert_eq!(commit_url(&version), format!("{RELEASES}/{tag}/COMMIT"));
        assert_eq!(
            binary_sums_url(&version),
            format!("{RELEASES}/{tag}/BINARY-SHA256SUMS")
        );
    }
    Ok(())
}

#[test]
fn check_tag_accepts_only_the_rust_ci_v_or_tools_v_tag_of_its_own_version() {
    for tag in ["rust-ci-v0.1.79", "tools-v0.1.79"] {
        assert_eq!(
            velnor_gen::check_tag("0.1.79", tag),
            Ok(format!("tag {tag} names version 0.1.79"))
        );
    }
    for tag in [
        "rust-ci-v0.1.80",
        "rust-ci-v0.1.79-rc1",
        "rust-ci-v0.1.79 ",
        "tools-v0.1.80",
        "tools-v0.1.79-rc1",
        "tools-v0.1.79 ",
        "v0.1.79",
        "vv0.1.79",
        "runner-v0.1.79",
        "actions-v0.1.79",
        "velnor-ci-v0.1.79",
        "velnor-gen-v0.1.79",
        "rust-ci-0.1.79",
        "0.1.79",
        "",
    ] {
        assert!(
            velnor_gen::check_tag("0.1.79", tag).is_err(),
            "{tag:?} was accepted"
        );
    }
}
