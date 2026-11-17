// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, expected_with_notes};
use velnor_model::RepoPath;
use velnor_rust::DetectError;

use crate::facts::TestResult;
use crate::pipeline::view;

fn with(name: &str) -> Result<velnor_model::RepoView, Box<dyn std::error::Error>> {
    let file = RepoPath::new(name)?;
    let mut repo = view(&["Cargo.toml"], None)?;
    repo.tracked_paths.insert(file);
    crate::facts::lock_pinned_tools(&mut repo)?;
    Ok(repo)
}

#[test]
fn a_toml_name_that_mise_does_not_load_is_not_a_mise_config() -> TestResult {
    for name in ["mise-en-place.toml", ".config/miscellany.toml", "rtxt.toml"] {
        let result = expected_with_notes(&with(name)?);
        assert!(result.is_ok(), "{name}: {:?}", result.err());
    }
    Ok(())
}

#[test]
fn a_name_that_mise_loads_and_velnor_does_not_model_is_still_refused() -> TestResult {
    for name in [
        ".tool-versions",
        "mise.local.toml",
        "mise.ci.toml",
        ".mise.local.toml",
        "rtx.toml",
        ".rtx.toml",
        ".config/mise.toml",
        ".config/mise/config.toml",
        "mise/config.toml",
        ".mise/config.toml",
    ] {
        let file = RepoPath::new(name)?;
        assert!(
            matches!(
                expected_with_notes(&with(name)?),
                Err(GenerateError::Detect(DetectError::Invalid { ref path, .. })) if *path == file
            ),
            "{name}"
        );
    }
    Ok(())
}
