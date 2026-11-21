// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::workflow;
use crate::private_sources::SECTION;
use crate::test_groups::{DATABASE, view_with};

#[test]
fn test_groups_with_private_sources_are_refused_naming_both_keys() -> TestResult {
    let config = format!("{DATABASE}\n{SECTION}");
    let error = workflow(&view_with(&["Cargo.toml"], &config, "0.1.90")?)
        .err()
        .ok_or("the groups were accepted and then dropped from the workflow")?;
    let text = error.to_string();
    assert!(
        text.contains("[[test_groups]]") && text.contains("[private_sources]"),
        "{text}"
    );
    Ok(())
}

const GIT_LOCK: &str = "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"shared\"\nversion = \"1.0.0\"\nsource = \"git+https://github.com/acme/shared-lib?branch=main#0123456789abcdef\"\n";

fn with_private_lock(
    config: &str,
    helper: &str,
) -> Result<velnor_model::RepoView, Box<dyn std::error::Error>> {
    let mut repo = view_with(&["Cargo.toml"], config, helper)?;
    repo.files.insert(
        crate::facts::path("Cargo.lock")?,
        GIT_LOCK.as_bytes().to_vec(),
    );
    Ok(repo)
}

#[test]
fn test_groups_are_refused_with_private_sources_at_every_helper_that_keys_the_private_base()
-> TestResult {
    let config = format!("{DATABASE}\n{SECTION}");
    for helper in ["0.1.111", "0.1.112"] {
        let error = workflow(&with_private_lock(&config, helper)?)
            .err()
            .ok_or_else(|| format!("{helper}: groups and private sources were both rendered"))?;
        let text = error.to_string();
        assert!(
            text.contains("[[test_groups]]") && text.contains("[private_sources]"),
            "{helper}: {text}"
        );
    }
    Ok(())
}

#[test]
fn the_archive_lane_of_test_groups_restores_only_shared_keys() -> TestResult {
    let text = workflow(&with_private_lock(DATABASE, "0.1.112")?)?;
    assert!(text.contains("velnor-src-"), "{text}");
    assert!(text.contains("velnor-tgt-"), "{text}");
    assert!(!text.contains("velnor-private-"), "{text}");
    Ok(())
}
