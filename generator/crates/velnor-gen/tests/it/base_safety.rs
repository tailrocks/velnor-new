// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoPath;
use crate::base_save::{jobs, pinned, step};
use crate::facts::TestResult;
use crate::pipeline::workflow;

#[test]
fn cache_targets_cannot_overlap_tracked_files_or_repository_control_paths() -> TestResult {
    for target in ["src", ".cargo", ".git", ".git/objects", "Cargo.lock", "src/cache"] {
        let mut view = pinned("0.1.81", "")?;
        for path in ["src/lib.rs", "src", ".cargo/config.toml"] {
            view.tracked_paths.insert(RepoPath::new(path)?);
        }
        view.files.insert(RepoPath::new(".cargo/config.toml")?, format!("[build]\ntarget-dir = {target:?}\n").into_bytes());
        let result = workflow(&view);
        assert!(result.is_err(), "accepted destructive target {target}");
    }
    Ok(())
}

#[test]
fn target_publication_waits_for_the_compile_input_audit_in_every_shape() -> TestResult {
    for shape in ["one-job", "lint-test", "groups"] {
        let config = format!("shape = {shape:?}\non_main = 'everything'\n");
        let all = jobs("0.1.81", &config)?;
        for (id, lane) in &all {
            if let Some(save) = lane.find("name: Save the target cache") {
                let audit = lane.find("name: velnor-ci inputs audit");
                assert!(audit.is_some_and(|index| index < save) || lane.contains("- rust-inputs-audit"), "{id} publishes before audit: {lane}");
                let saved = step(lane, "Save the target cache").ok_or("save")?;
                assert!(saved.contains("success()") || saved.contains("steps.audit.outcome == ''success''"), "{saved}");
            }
        }
    }
    Ok(())
}

#[test]
fn source_cache_misses_use_source_safety_independently_of_target_production() -> TestResult {
    let all = jobs("0.1.81", "")?;
    let lane = &all.iter().find(|(id, _)| id == "rust").ok_or("lane")?.1;
    let sources = step(lane, "Save the sources cache").ok_or("sources")?;
    assert!(sources.contains("steps.base.outputs.sources == ''true''"), "{sources}");
    assert!(!sources.contains("steps.base.outputs.save == ''true''"), "{sources}");
    Ok(())
}

#[test]
fn upload_failures_are_visible_to_the_cache_check() -> TestResult {
    let all = jobs("0.1.81", "")?;
    let lane = &all.iter().find(|(id, _)| id == "rust").ok_or("lane")?.1;
    for name in ["Save the target cache", "Save the sources cache"] {
        let save = step(lane, name).ok_or("save")?;
        assert!(save.contains("fail-on-cache-error: true"), "{save}");
    }
    Ok(())
}
