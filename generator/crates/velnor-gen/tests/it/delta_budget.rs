// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::facts::TestResult;
use crate::pipeline::view;
use velnor_gen::expected_with_notes;
use velnor_model::{RepoPath, RepoView};

const WORKFLOW: &str = ".github/workflows/rust-ci.yml";

fn long_named_roots(count: usize) -> Vec<String> {
    (0..count)
        .map(|index| {
            if index == 0 {
                "Cargo.toml".to_owned()
            } else {
                format!("crate-with-a-long-directory-name-{index}/Cargo.toml")
            }
        })
        .collect()
}

fn with_helper(mut repo: RepoView, helper: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let lock = RepoPath::new(".velnor/rust-ci.lock")?;
    let text = String::from_utf8(repo.files.remove(&lock).ok_or("no helper lock")?)?;
    repo.files
        .insert(lock, text.replace("0.1.82", helper).into_bytes());
    Ok(repo)
}

#[test]
fn a_workflow_that_does_not_fit_keeps_its_checks_and_reports_the_delta_layer_it_left_out()
-> TestResult {
    let manifests = long_named_roots(20);
    let roots: Vec<&str> = manifests.iter().map(String::as_str).collect();
    let (files, notes) = expected_with_notes(&with_helper(view(&roots, None)?, "0.1.86")?)?;
    let text = files.get(&RepoPath::new(WORKFLOW)?).ok_or("no workflow")?;
    assert!(
        text.len() <= velnor_render::MAX_WORKFLOW_BYTES,
        "{} bytes",
        text.len()
    );
    assert!(
        text.contains("  rust-crate_with_a_long_directory_name_19:"),
        "{text}"
    );
    assert!(text.contains("  required:"), "{text}");
    assert!(!text.contains("velnor-ci delta"), "{text}");
    assert!(!text.contains("velnor-dlt-"), "{text}");
    assert!(!files.contains_key(&velnor_render::DELTA_CLEANUP_PATH));
    let note = notes
        .iter()
        .find(|note| note.contains("delta"))
        .ok_or("no note about the delta layer")?;
    let size: usize = note
        .split("would be ")
        .nth(1)
        .and_then(|rest| rest.split(" bytes").next())
        .and_then(|size| size.parse().ok())
        .ok_or("no byte count in the note")?;
    assert!(size > velnor_render::MAX_WORKFLOW_BYTES, "{note}");
    assert!(
        note.contains(&velnor_render::MAX_WORKFLOW_BYTES.to_string()),
        "{note}"
    );
    Ok(())
}

#[test]
fn a_workflow_that_fits_keeps_the_delta_layer_and_reports_nothing_about_it() -> TestResult {
    let (files, notes) = expected_with_notes(&view(&["Cargo.toml", "lab/Cargo.toml"], None)?)?;
    let text = files.get(&RepoPath::new(WORKFLOW)?).ok_or("no workflow")?;
    assert!(text.contains("velnor-ci delta"), "{text}");
    assert!(text.contains("velnor-dlt-"), "{text}");
    assert!(files.contains_key(&velnor_render::DELTA_CLEANUP_PATH));
    assert!(
        !notes.iter().any(|note| note.contains("delta")),
        "{notes:?}"
    );
    Ok(())
}
