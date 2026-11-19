// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::expected_with_notes;
use velnor_model::RepoPath;

use crate::facts::TestResult;
use crate::pipeline::view;

#[test]
fn unforced_compiler_and_cargo_environment_prefixes_are_rejected() -> TestResult {
    for name in ["CLIPPY_CUSTOM", "RUST_CUSTOM", "CARGO_CUSTOM"] {
        let mut repo = view(&["Cargo.toml"], None)?;
        let config = RepoPath::new(".cargo/config.toml")?;
        repo.tracked_paths.insert(config.clone());
        repo.files
            .insert(config, format!("[env]\n{name} = \"value\"\n").into_bytes());
        let result = expected_with_notes(&repo);
        assert!(result.is_err(), "{name} was accepted");
    }
    Ok(())
}
