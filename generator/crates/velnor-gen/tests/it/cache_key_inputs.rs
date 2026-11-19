// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache_key_defects::with_config;
use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

#[test]
fn a_config_toml_shadowed_by_config_is_ignored_for_every_key() -> TestResult {
    let mut shadowed = view(&["Cargo.toml"], None)?;
    with_config(&mut shadowed, ".cargo/config", "[build]\njobs = 2\n")?;
    with_config(
        &mut shadowed,
        ".cargo/config.toml",
        "[build]\ntarget-dir = \"ignored\"\n",
    )?;
    let text = workflow(&shadowed)?;
    assert!(text.contains("path: target\n"), "{text}");
    assert!(!text.contains("path: ignored\n"), "{text}");
    Ok(())
}

#[test]
fn a_shadowed_config_toml_naming_an_unknown_build_target_does_not_block_generation() -> TestResult {
    let mut shadowed = view(&["Cargo.toml"], None)?;
    with_config(&mut shadowed, ".cargo/config", "[build]\njobs = 2\n")?;
    with_config(
        &mut shadowed,
        ".cargo/config.toml",
        "[build]\ntarget = \"unknown-target\"\n",
    )?;
    workflow(&shadowed)?;
    Ok(())
}

#[test]
fn build_dir_and_placeholder_target_dirs_fail_generation() -> TestResult {
    for body in [
        "[build]\nbuild-dir = \"out\"\n",
        "[build]\ntarget-dir = \"{workspace-root}/out\"\n",
        "[build]\ntarget-dir = \"{cargo-cache-home}/t\"\n",
        "[build]\ntarget-dir = \"~\"\n",
        "[build]\ntarget-dir = \"~/out\"\n",
    ] {
        let mut one = view(&["Cargo.toml"], None)?;
        with_config(&mut one, ".cargo/config.toml", body)?;
        let error = workflow(&one)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            error.contains(".cargo/config.toml") && error.contains("build"),
            "{body}: {error}"
        );
    }
    Ok(())
}

#[test]
fn a_leading_tilde_in_a_hashed_path_is_read_literally() -> TestResult {
    let hash = crate::shared_sources::locks_hash(&["~x/Cargo.toml"])?;
    assert!(hash.contains("'./~x/Cargo.lock'"), "{hash}");
    Ok(())
}
