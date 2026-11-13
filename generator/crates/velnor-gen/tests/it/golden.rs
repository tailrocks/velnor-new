// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use similar::TextDiff;
use velnor_gen::expected_files;
use velnor_gen::io::SAMPLE_REPOSITORY;
use velnor_model::{RepoPath, RepoView};
use velnor_rust::{reduce_metadata, wanted_files};

type TestResult = Result<(), Box<dyn Error>>;

fn samples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../samples")
}

fn files_under(directory: &Path) -> Result<BTreeMap<RepoPath, Vec<u8>>, Box<dyn Error>> {
    let mut files = BTreeMap::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(current) = pending.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries {
            let path = entry?.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                let relative = path
                    .strip_prefix(directory)?
                    .to_str()
                    .ok_or("a sample path is not UTF-8")?
                    .to_owned();
                files.insert(RepoPath::new(relative)?, fs::read(&path)?);
            }
        }
    }
    Ok(files)
}

fn load(sample: &Path) -> Result<RepoView, Box<dyn Error>> {
    let recorded = files_under(&sample.join("view"))?;
    let mut tracked_paths: BTreeSet<RepoPath> = recorded.keys().cloned().collect();
    for line in fs::read_to_string(sample.join("manifests.txt"))?.lines() {
        tracked_paths.insert(RepoPath::new(line)?);
    }
    let files = wanted_files(&tracked_paths)
        .into_iter()
        .filter_map(|path| recorded.get(&path).map(|bytes| (path, bytes.clone())))
        .collect();
    let mut rust_metadata = BTreeMap::new();
    for json in files_under(&sample.join("cargo-metadata"))?.values() {
        let (workspace, metadata) = reduce_metadata(Path::new(SAMPLE_REPOSITORY), json)?;
        rust_metadata.insert(workspace, metadata);
    }
    Ok(RepoView {
        files,
        tracked_paths,
        rust_metadata,
    })
}

fn rewrite(expected_dir: &Path, generated: &BTreeMap<RepoPath, String>) -> TestResult {
    if expected_dir.exists() {
        fs::remove_dir_all(expected_dir)?;
    }
    for (path, content) in generated {
        let file = expected_dir.join(path.as_str());
        fs::create_dir_all(file.parent().ok_or("no parent")?)?;
        fs::write(file, content)?;
    }
    Ok(())
}

fn mismatches(
    name: &str,
    generated: &BTreeMap<RepoPath, String>,
    on_disk: &BTreeMap<RepoPath, Vec<u8>>,
) -> Vec<String> {
    let paths: BTreeSet<&RepoPath> = generated.keys().chain(on_disk.keys()).collect();
    paths
        .into_iter()
        .filter_map(|path| {
            let want = on_disk.get(path);
            let have = generated.get(path);
            (want.map(Vec::as_slice) != have.map(String::as_bytes)).then(|| {
                let want_text = want.map(|b| String::from_utf8_lossy(b).into_owned());
                let have_text = have.cloned();
                let diff = TextDiff::from_lines(
                    want_text.as_deref().unwrap_or_default(),
                    have_text.as_deref().unwrap_or_default(),
                )
                .unified_diff()
                .header(&format!("expected/{path}"), &format!("generated/{path}"))
                .to_string();
                format!("sample {name}\n{diff}")
            })
        })
        .collect()
}

#[test]
fn every_sample_repository_renders_exactly_its_expected_files() -> TestResult {
    let update = std::env::var_os("UPDATE_GOLDENS").is_some();
    let mut samples = fs::read_dir(samples())?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    samples.retain(|path| path.is_dir());
    samples.sort();
    assert!(!samples.is_empty(), "no sample recorded");
    let mut failures = Vec::new();
    for sample in &samples {
        let generated = expected_files(&load(sample)?)?;
        let expected_dir = sample.join("expected");
        if update {
            rewrite(&expected_dir, &generated)?;
        } else {
            let name = sample.file_name().unwrap_or_default().to_string_lossy();
            failures.extend(mismatches(&name, &generated, &files_under(&expected_dir)?));
        }
    }
    assert!(
        failures.is_empty(),
        "run `UPDATE_GOLDENS=1 cargo nextest run` and review the diff of expected/\n{}",
        failures.join("\n")
    );
    Ok(())
}

#[test]
fn a_missing_or_byte_different_expected_file_is_a_mismatch() -> TestResult {
    let path = RepoPath::new(".github/workflows/rust-ci.yml")?;
    let empty = BTreeMap::from([(path.clone(), String::new())]);
    assert_eq!(mismatches("s", &empty, &BTreeMap::new()).len(), 1);
    assert_eq!(
        mismatches(
            "s",
            &BTreeMap::new(),
            &BTreeMap::from([(path.clone(), vec![])])
        )
        .len(),
        1
    );
    let text = BTreeMap::from([(path.clone(), "\u{fffd}".to_owned())]);
    let invalid = BTreeMap::from([(path.clone(), vec![0xff])]);
    assert_eq!(mismatches("s", &text, &invalid).len(), 1);
    let same = BTreeMap::from([(path, b"a\n".to_vec())]);
    let generated = BTreeMap::from([(
        RepoPath::new(".github/workflows/rust-ci.yml")?,
        "a\n".to_owned(),
    )]);
    assert_eq!(mismatches("s", &generated, &same).len(), 0);
    Ok(())
}

#[test]
fn every_expected_workflow_is_an_input_of_the_generator_samples_check() -> TestResult {
    let config: toml::Table = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../.velnor/config.toml"),
    )?
    .parse()?;
    let listed: BTreeSet<String> = config
        .get("checks")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|check| check.get("id").and_then(toml::Value::as_str) == Some("generator-samples"))
        .filter_map(|check| check.get("inputs").and_then(toml::Value::as_array))
        .flatten()
        .filter_map(toml::Value::as_str)
        .filter(|input| input.starts_with("generator/samples/"))
        .map(str::to_owned)
        .collect();
    let mut present = BTreeSet::new();
    for sample in fs::read_dir(samples())? {
        let sample = sample?.path();
        for path in files_under(&sample.join("expected"))?.keys() {
            if matches!(
                Path::new(path.as_str())
                    .extension()
                    .and_then(|e| e.to_str()),
                Some("yml" | "yaml")
            ) {
                let name = sample.file_name().unwrap_or_default().to_string_lossy();
                present.insert(format!("generator/samples/{name}/expected/{path}"));
            }
        }
    }
    assert_eq!(present, listed);
    Ok(())
}
