// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::expected_with_notes;
use velnor_model::RepoView;
use velnor_rust::TIMINGS_PATH;

use crate::pipeline::view;

type TestResult = Result<(), Box<dyn Error>>;

const BOTH_ROOTS_TIMED: &str = r#"{"Cargo.toml": {"lint_build_seconds": 10, "test_build_seconds": 20, "test_seconds": 4}, "lab/Cargo.toml": {"lint_build_seconds": 10, "test_build_seconds": 20, "test_seconds": 4}}"#;

fn repo(timings: Option<&str>, config: &str) -> Result<RepoView, Box<dyn Error>> {
    let mut repo = view(&["Cargo.toml", "lab/Cargo.toml"], Some(config.as_bytes()))?;
    if let Some(timings) = timings {
        repo.tracked_paths.insert(TIMINGS_PATH);
        repo.files.insert(TIMINGS_PATH, timings.as_bytes().to_vec());
    }
    Ok(repo)
}

fn auto_notes(repo: &RepoView) -> Result<Vec<String>, Box<dyn Error>> {
    let (_, notes) = expected_with_notes(repo)?;
    Ok(notes
        .into_iter()
        .filter(|note| note.contains("shape = \"auto\""))
        .collect())
}

#[test]
fn auto_without_timings_says_which_shape_it_picked_and_how_to_choose_or_measure() -> TestResult {
    let notes = auto_notes(&repo(None, "")?)?;
    let [note] = notes.as_slice() else {
        return Err(format!("expected one auto note, got {notes:?}").into());
    };
    for expected in [
        "one-job",
        TIMINGS_PATH.as_str(),
        "shape = \"lint-test\"",
        "velnor-ci timings",
    ] {
        assert!(note.contains(expected), "{expected} missing from {note}");
    }
    Ok(())
}

#[test]
fn auto_with_timings_for_every_root_gives_no_auto_note() -> TestResult {
    let notes = auto_notes(&repo(Some(BOTH_ROOTS_TIMED), "")?)?;
    assert_eq!(notes, Vec::<String>::new());
    Ok(())
}

#[test]
fn an_explicit_shape_gives_no_auto_note_even_without_timings() -> TestResult {
    for shape in ["one-job", "lint-test", "groups"] {
        let config = format!("shape = \"{shape}\"\n");
        let notes = auto_notes(&repo(None, &config)?)?;
        assert_eq!(notes, Vec::<String>::new(), "{shape}");
    }
    Ok(())
}
