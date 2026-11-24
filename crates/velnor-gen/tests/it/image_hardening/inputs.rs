// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use velnor_gen::io::{SAMPLE_REPOSITORY, recording_of, write_sample};
use velnor_gen::{GenerateError, ImageError, expected_files};
use velnor_model::RepoPath;

use crate::facts::TestResult;
use crate::image_workflow::{edited, sample};
use crate::io::Scratch;

fn migrations() -> Result<RepoPath, Box<dyn std::error::Error>> {
    Ok(RepoPath::new("migrations/0001_init.sql")?)
}

#[test]
fn an_image_input_that_names_no_tracked_path_is_refused() -> TestResult {
    let tracked = sample("image-ghcr")?;
    assert!(expected_files(&tracked).is_ok());
    let mut missing = tracked.clone();
    missing.tracked_paths.remove(&migrations()?);
    assert!(matches!(
        expected_files(&missing),
        Err(GenerateError::Image(ImageError::InputUntracked { .. }))
    ));
    let mistyped = edited("image-ghcr", |text| {
        text.replace("inputs = [\"migrations\"]", "inputs = [\"migration\"]")
    })?;
    assert!(matches!(
        expected_files(&mistyped),
        Err(GenerateError::Image(ImageError::InputUntracked { .. }))
    ));
    let mut single = edited("image-ghcr", |text| {
        text.replace("inputs = [\"migrations\"]", "inputs = [\"data.json\"]")
    })?;
    assert!(expected_files(&single).is_err());
    single.tracked_paths.insert(RepoPath::new("data.json")?);
    assert!(expected_files(&single).is_ok());
    Ok(())
}

#[test]
fn an_image_input_behind_a_symbolic_link_or_a_submodule_is_refused() -> TestResult {
    let mut links = sample("image-ghcr")?;
    links.symlinks.insert(RepoPath::new("migrations")?);
    assert!(matches!(
        expected_files(&links),
        Err(GenerateError::Image(ImageError::InputLinked { .. }))
    ));
    let mut nested = edited("image-ghcr", |text| {
        text.replace(
            "inputs = [\"migrations\"]",
            "inputs = [\"migrations/0001_init.sql\"]",
        )
    })?;
    assert!(expected_files(&nested).is_ok());
    nested.symlinks.insert(RepoPath::new("migrations")?);
    assert!(matches!(
        expected_files(&nested),
        Err(GenerateError::Image(ImageError::InputLinked { .. }))
    ));
    let mut module = sample("image-ghcr")?;
    module.gitlinks.insert(RepoPath::new("migrations")?);
    assert!(matches!(
        expected_files(&module),
        Err(GenerateError::Image(ImageError::InputLinked { .. }))
    ));
    Ok(())
}

#[test]
fn a_recorded_sample_keeps_the_tracked_files_of_its_image_inputs() -> TestResult {
    let scratch = Scratch::new("record-image-inputs")?;
    let source = crate::golden::samples().join("image-ghcr");
    let view = crate::golden::load(&source)?;
    let metadata = BTreeMap::from([(
        RepoPath::new("Cargo.toml")?,
        std::fs::read(source.join("cargo-metadata/Cargo.toml.json"))?,
    )]);
    let recording = recording_of(
        Path::new(SAMPLE_REPOSITORY),
        view,
        metadata,
        &mut Vec::new(),
    )?;
    write_sample(&scratch.repo(), "demo", &recording)?;
    let recorded = crate::golden::load(&scratch.repo().join("generator/samples/demo"))?;
    assert!(recorded.tracked_paths.contains(&migrations()?));
    Ok(())
}
