// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::RepoPath;
use velnor_rust::{DetectError, Reason, repo_profile};

use crate::facts::TestResult;

fn files(
    entries: &[(&str, &str)],
) -> Result<BTreeMap<RepoPath, Vec<u8>>, Box<dyn std::error::Error>> {
    entries
        .iter()
        .map(|(path, text)| Ok((RepoPath::new(*path)?, text.as_bytes().to_vec())))
        .collect()
}

#[test]
fn a_cargo_config_that_includes_another_file_cannot_be_hashed_as_having_no_profile() -> TestResult {
    let included = |opt: &str| {
        files(&[
            (".cargo/config.toml", "include = [\"prof.toml\"]\n"),
            (
                ".cargo/prof.toml",
                &format!("[profile.dev]\nopt-level = {opt}\n"),
            ),
        ])
    };
    for opt in ["3", "0"] {
        assert!(
            matches!(
                repo_profile(&included(opt)?),
                Err(DetectError::Invalid { ref path, reason: Reason::CargoConfigInclude })
                    if path.as_str() == ".cargo/config.toml"
            ),
            "{opt}"
        );
    }
    let nested = files(&[("lab/.cargo/config", "include = \"prof.toml\"\n")])?;
    assert!(matches!(
        repo_profile(&nested),
        Err(DetectError::Invalid {
            reason: Reason::CargoConfigInclude,
            ..
        })
    ));
    Ok(())
}
