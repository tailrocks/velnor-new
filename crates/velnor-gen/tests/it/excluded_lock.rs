// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! An excluded nested workspace keeps its Cargo.lock out of the sources key and the prefetch,
//! so a job that runs cargo against it fails offline. Generation names that case.

use std::error::Error;

use velnor_gen::{GenerateError, expected_with_notes};
use velnor_model::RepoPath;

use crate::pipeline::view;

type TestResult = Result<(), Box<dyn Error>>;

const EXCLUDE: &[u8] = b"exclude = [\"crates/schema-gen/**\"]\n";

/// A view with the root workspace and an excluded nested workspace `crates/schema-gen`.
fn excluded_view() -> Result<velnor_model::RepoView, Box<dyn Error>> {
    view(
        &["Cargo.toml", "crates/schema-gen/Cargo.toml"],
        Some(EXCLUDE),
    )
}

/// Adds a `.velnor/workflows.toml` with one job whose single run step is `step`.
fn with_step(
    mut view: velnor_model::RepoView,
    step: &str,
) -> Result<velnor_model::RepoView, Box<dyn Error>> {
    let text = format!(
        "[[workflow]]\nfile = \"ci\"\nname = \"CI\"\n\n[[workflow.job]]\nid = \"schema\"\n\n[[workflow.job.step]]\nname = \"Schema\"\n{step}\n"
    );
    view.files
        .insert(RepoPath::new(".velnor/workflows.toml")?, text.into_bytes());
    Ok(view)
}

fn refusal(view: &velnor_model::RepoView) -> Result<String, Box<dyn Error>> {
    match expected_with_notes(view) {
        Err(error @ GenerateError::ExcludedLockRun(_)) => Ok(error.to_string()),
        Err(error) => Err(format!("unexpected error: {error}").into()),
        Ok(_) => Err("expected a refusal".into()),
    }
}

fn assert_names_fix(message: &str) {
    assert!(
        message.contains("crates/schema-gen/Cargo.lock"),
        "names the lockfile: {message}"
    );
    assert!(
        message.contains("remove it from the exclude list")
            && message.contains("stop running cargo against it in CI"),
        "names the fix: {message}"
    );
}

#[test]
fn a_run_step_with_manifest_path_into_an_excluded_root_is_refused() -> TestResult {
    let view = with_step(
        excluded_view()?,
        "run = \"cargo test --manifest-path crates/schema-gen/Cargo.toml\"",
    )?;
    assert_names_fix(&refusal(&view)?);
    Ok(())
}

#[test]
fn a_run_step_with_cd_into_an_excluded_root_is_refused() -> TestResult {
    let view = with_step(
        excluded_view()?,
        "run = \"cd crates/schema-gen; cargo test\"",
    )?;
    assert_names_fix(&refusal(&view)?);
    Ok(())
}

#[test]
fn a_step_working_directory_in_an_excluded_root_is_refused() -> TestResult {
    let view = with_step(
        excluded_view()?,
        "working_directory = \"crates/schema-gen\"\nrun = \"cargo test\"",
    )?;
    assert_names_fix(&refusal(&view)?);
    Ok(())
}

#[test]
fn an_excluded_root_that_no_job_reaches_is_silent() -> TestResult {
    let view = with_step(
        excluded_view()?,
        "run = \"cargo test --manifest-path Cargo.toml\"",
    )?;
    let (_, notes) = expected_with_notes(&view).map_err(|e| e.to_string())?;
    assert!(
        notes.iter().all(|note| !note.contains("schema-gen")),
        "{notes:?}"
    );
    Ok(())
}

#[test]
fn a_mise_task_into_an_excluded_root_is_a_warning_not_a_refusal() -> TestResult {
    let mut view = excluded_view()?;
    let mise = RepoPath::new("mise.toml")?;
    let mut text = view.files.get(&mise).ok_or("no mise.toml")?.clone();
    text.extend_from_slice(
        b"\n[tasks.schema]\nrun = \"cargo test --manifest-path crates/schema-gen/Cargo.toml\"\n",
    );
    view.files.insert(mise, text);
    let (_, notes) = expected_with_notes(&view).map_err(|e| e.to_string())?;
    let warning = notes
        .iter()
        .find(|note| note.contains("crates/schema-gen/Cargo.lock"))
        .ok_or("no warning for the mise task")?;
    assert!(
        warning.contains("stop running cargo against it in CI"),
        "{warning}"
    );
    Ok(())
}
