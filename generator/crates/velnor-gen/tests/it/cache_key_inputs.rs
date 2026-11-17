// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CacheKey, CacheRoot, Host, RepoPath};
use velnor_render::render_key;

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
    let key = render_key(&CacheKey::Sources {
        host: Host,
        root: CacheRoot::new(&RepoPath::new("~x/Cargo.toml")?, RepoPath::new("target")?),
    })
    .key;
    assert!(key.contains("'./~x/Cargo.lock'"), "{key}");
    Ok(())
}

#[test]
fn target_cache_directories_cannot_overlap_tracked_inputs() -> TestResult {
    let mut failures = Vec::new();
    for (target, tracked) in [
        (None, "target/.velnor/inputs.json"),
        (Some("out"), "out/source.rs"),
        (Some("source/cache"), "source"),
        (None, "target"),
    ] {
        let mut repo = view(&["Cargo.toml"], None)?;
        if let Some(target) = target {
            with_config(
                &mut repo,
                ".cargo/config.toml",
                &format!("[build]\ntarget-dir = {target:?}\n"),
            )?;
        }
        repo.tracked_paths.insert(RepoPath::new(tracked)?);
        let error = workflow(&repo)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        if !error.contains("target") || !error.contains(tracked) {
            failures.push(format!("{target:?} {tracked}: {error}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

#[test]
fn target_cache_directories_allow_component_siblings() -> TestResult {
    let mut repo = view(&["Cargo.toml"], None)?;
    repo.tracked_paths
        .insert(RepoPath::new("target-backup/source.rs")?);
    assert!(workflow(&repo)?.contains("path: target\n"));
    Ok(())
}
