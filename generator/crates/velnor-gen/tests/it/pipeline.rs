// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_gen::expected_files;
use velnor_model::{PackageName, RepoPath, RepoView, RustMetadata, RustPackage};
use velnor_rust::DetectError;

type TestResult = Result<(), Box<dyn Error>>;

const WORKFLOW: &str = ".github/workflows/rust-ci.yml";

fn view(roots: &[&str], config: Option<&[u8]>) -> Result<RepoView, Box<dyn Error>> {
    let mut view = RepoView::default();
    for (index, root) in roots.iter().enumerate() {
        let manifest = RepoPath::new(*root)?;
        view.tracked_paths.insert(manifest.clone());
        let member = RustPackage {
            name: PackageName::new(format!("package-{index}"))?,
            manifest: manifest.clone(),
            dependencies: Vec::new(),
            targets: Vec::new(),
        };
        view.rust_metadata.insert(
            manifest,
            RustMetadata {
                members: vec![member],
            },
        );
    }
    if let Some(bytes) = config {
        let path = RepoPath::new(".velnor/rust-ci.toml")?;
        view.tracked_paths.insert(path.clone());
        view.files.insert(path, bytes.to_vec());
    }
    Ok(view)
}

fn workflow(view: &RepoView) -> Result<String, Box<dyn Error>> {
    let mut files = expected_files(view)?;
    let text = files
        .remove(&RepoPath::new(WORKFLOW)?)
        .ok_or("the workflow was not rendered")?;
    if !files.is_empty() {
        return Err(format!("unexpected files: {:?}", files.keys()).into());
    }
    Ok(text)
}

#[test]
fn two_cargo_roots_render_one_format_job_with_a_step_per_root() -> TestResult {
    let files = expected_files(&view(&["Cargo.toml", "lab/Cargo.toml"], None)?)?;
    let expected = "\
name: Rust CI
on:
  pull_request: {}
  workflow_dispatch: {}
permissions:
  contents: read
concurrency:
  group: '${{ github.workflow }}-${{ github.ref }}'
  cancel-in-progress: true
jobs:
  format:
    runs-on: ubuntu-latest
    timeout-minutes: 30
    steps:
      - name: Check out
        uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          fetch-depth: 1
          persist-credentials: false
      - name: Format Cargo.toml
        run: cargo fmt --all --check --manifest-path Cargo.toml
      - name: Format lab/Cargo.toml
        run: cargo fmt --all --check --manifest-path lab/Cargo.toml
";
    assert_eq!(
        files,
        BTreeMap::from([(RepoPath::new(WORKFLOW)?, expected.to_owned())])
    );
    Ok(())
}

#[test]
fn the_config_file_sets_runner_time_limit_and_excluded_roots() -> TestResult {
    let config = b"runner = \"self-hosted\"\ntimeout_minutes = 12\nexclude = [\"lab/\"]\n";
    let text = workflow(&view(&["Cargo.toml", "lab/Cargo.toml"], Some(config))?)?;
    assert!(text.contains("    runs-on: self-hosted\n    timeout-minutes: 12\n"));
    assert!(text.contains("run: cargo fmt --all --check --manifest-path Cargo.toml\n"));
    assert!(!text.contains("lab/Cargo.toml"));
    Ok(())
}

#[test]
fn values_yaml_or_the_shell_would_misread_are_quoted() -> TestResult {
    let text = workflow(&view(
        &["it's crates/Cargo.toml"],
        Some(b"runner = \"on\"\n"),
    )?)?;
    assert!(text.contains("    runs-on: 'on'\n"));
    assert!(text.contains(
        "        run: 'cargo fmt --all --check --manifest-path ''it''\\''''s crates/Cargo.toml'''\n"
    ));
    Ok(())
}

#[test]
fn expression_syntax_in_repository_values_is_never_evaluated_by_github() -> TestResult {
    let root = "a${{ github.event.pull_request.title }}/Cargo.toml";
    let config = b"runner = \"${{ secrets.TOKEN }}\"\n";
    let text = workflow(&view(&[root], Some(config))?)?;
    assert!(text.contains("    runs-on: '${{ ''${{'' }} secrets.TOKEN }}'\n"));
    assert!(text.contains(
        "      - name: 'Format a${{ ''${{'' }} github.event.pull_request.title }}/Cargo.toml'\n"
    ));
    assert!(text.contains(
        "        run: 'cargo fmt --all --check --manifest-path ''a${{ ''${{'' }} github.event.pull_request.title }}/Cargo.toml'''\n"
    ));
    assert!(text.contains("  group: '${{ github.workflow }}-${{ github.ref }}'\n"));
    Ok(())
}

#[test]
fn an_invalid_config_is_rejected() -> TestResult {
    let cases: [&[u8]; 6] = [
        b"runnr = \"ubuntu-latest\"\n",
        b"runner = \n",
        b"runner = \"\xff\"\n",
        b"timeout_minutes = 0\n",
        b"timeout_minutes = 361\n",
        b"exclude = [\"../lab\"]\n",
    ];
    for config in cases {
        let result = expected_files(&view(&["Cargo.toml"], Some(config))?);
        assert!(
            matches!(result, Err(DetectError::Config(_))),
            "{:?} gave {result:?}",
            String::from_utf8_lossy(config)
        );
    }
    Ok(())
}

#[test]
fn a_repository_without_a_cargo_root_is_rejected() -> TestResult {
    let empty = expected_files(&view(&[], None)?);
    let excluded = expected_files(&view(&["lab/Cargo.toml"], Some(b"exclude = [\"lab\"]\n"))?);
    assert_eq!(empty, Err(DetectError::NoRustRoot));
    assert_eq!(excluded, Err(DetectError::NoRustRoot));
    Ok(())
}
