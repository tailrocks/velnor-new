// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::{GenerateError, expected_with_notes};
use velnor_model::{
    PackageName, RepoPath, RepoView, RustMetadata, RustPackage, Target, TargetKind, TargetName,
};
use velnor_rust::{CONFIG_PATH, DetectError};

type TestResult = Result<(), Box<dyn Error>>;

pub(crate) const HELPER_LOCK: &[u8] =
    include_bytes!("../../../../samples/helper-lock/view/.velnor/rust-ci.lock");

const WORKFLOW: &str = ".github/workflows/rust-ci.yml";

pub(crate) fn view(roots: &[&str], config: Option<&[u8]>) -> Result<RepoView, Box<dyn Error>> {
    let mut view = RepoView::default();
    for (index, root) in roots.iter().enumerate() {
        let manifest = RepoPath::new(*root)?;
        view.tracked_paths
            .insert(manifest.sibling(&RepoPath::new("Cargo.lock")?));
        view.tracked_paths.insert(manifest.clone());
        let member = RustPackage {
            name: PackageName::new(format!("package-{index}"))?,
            version: "0.1.0".to_owned(),
            manifest: manifest.clone(),
            dependencies: Vec::new(),
            targets: vec![Target {
                name: TargetName::new("it")?,
                kinds: vec![TargetKind::Test],
                tests: true,
                doctest: false,
                source: None,
                requires_features: false,
            }],
        };
        view.rust_metadata.insert(
            manifest,
            RustMetadata {
                members: vec![member],
            },
        );
    }
    let mise = RepoPath::new("mise.toml")?;
    view.tracked_paths.insert(mise.clone());
    view.files.insert(
        mise,
        b"[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n"
            .to_vec(),
    );
    crate::facts::lock_pinned_tools(&mut view)?;
    let lock = RepoPath::new(".velnor/rust-ci.lock")?;
    view.tracked_paths.insert(lock.clone());
    view.files.insert(lock, HELPER_LOCK.to_vec());
    if let Some(bytes) = config {
        let path = RepoPath::new(".velnor/rust-ci.toml")?;
        view.tracked_paths.insert(path.clone());
        view.files.insert(path, bytes.to_vec());
    }
    Ok(view)
}

pub(crate) fn workflow(view: &RepoView) -> Result<String, Box<dyn Error>> {
    let mut locked = view.clone();
    crate::facts::lock_pinned_tools(&mut locked)?;
    let mut files = expected_with_notes(&locked).map(|(files, _)| files)?;
    let text = files
        .remove(&RepoPath::new(WORKFLOW)?)
        .ok_or("the workflow was not rendered")?;
    if !files.is_empty() {
        return Err(format!("unexpected files: {:?}", files.keys()).into());
    }
    Ok(text)
}

#[test]
fn two_cargo_roots_render_a_job_each_and_a_verdict_job_that_needs_both() -> TestResult {
    let text = workflow(&view(&["Cargo.toml", "lab/Cargo.toml"], None)?)?;
    let jobs: Vec<&str> = text
        .lines()
        .skip_while(|line| *line != "jobs:")
        .filter(|line| line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':'))
        .collect();
    assert_eq!(
        jobs,
        [
            "  rust-plan:",
            "  rust-root:",
            "  rust-lab:",
            "  rust-inputs-audit:",
            "  required:"
        ]
    );
    assert!(text.contains(
        "  rust-lab:\n    needs:\n      - rust-plan\n    if: '${{ needs.rust-plan.outputs.rust_lab_mode != ''none'' }}'\n"
    ));
    assert!(text.contains(
        "    needs:\n      - rust-plan\n      - rust-root\n      - rust-lab\n      - rust-inputs-audit\n    if: '${{ !cancelled() }}'\n"
    ));
    Ok(())
}

#[test]
fn the_config_file_sets_runner_time_limit_and_excluded_roots() -> TestResult {
    let config = b"runner = \"self-hosted\"\ntimeout_minutes = 12\nexclude = [\"lab/\"]\n";
    let text = workflow(&view(&["Cargo.toml", "lab/Cargo.toml"], Some(config))?)?;
    assert!(text.contains("    runs-on: self-hosted\n    timeout-minutes: 12\n"));
    assert!(text.contains(
        "      - name: Format\n        if: '${{ !cancelled() && steps.restored.outcome == ''success'' && steps.plan.outcome == ''success'' && steps.plan.outputs.rust_root_mode != ''none'' }}'\n        run: velnor-ci run -- cargo-fmt --all --check\n"
    ));
    assert!(!text.contains("lab"));
    Ok(())
}

#[test]
fn values_yaml_or_cargo_would_misread_are_quoted_or_kept_out_of_arguments() -> TestResult {
    let roots = [
        "-bad/Cargo.toml",
        "it's crates/Cargo.toml",
        "odd\u{ffff}/Cargo.toml",
    ];
    let text = workflow(&view(&roots, Some(b"runner = \"on\"\n"))?)?;
    assert!(text.contains("    runs-on: 'on'\n"));
    assert!(text.contains(
        "        working-directory: '-bad'\n        run: velnor-ci run -- cargo-fmt --all --check\n"
    ));
    assert!(text.contains(
        "        working-directory: 'it''s crates'\n        run: velnor-ci run -- cargo-fmt --all --check\n"
    ));
    assert!(text.contains(
        "        working-directory: \"odd\\uffff\"\n        run: velnor-ci run -- cargo-fmt --all --check\n"
    ));
    assert_eq!(
        text.matches("run: velnor-ci run -- cargo-fmt --all --check\n")
            .count(),
        3
    );
    for run in text
        .lines()
        .filter(|line| line.trim_start().starts_with("run:"))
    {
        assert!(
            !["bad", "crates", "odd"]
                .iter()
                .any(|root| run.contains(root)),
            "{run}"
        );
    }
    Ok(())
}

#[test]
fn expression_syntax_in_repository_values_is_never_evaluated_by_github() -> TestResult {
    let root = "a${{ github.event.pull_request.title }}/Cargo.toml";
    let config = b"runner = \"${{ secrets.TOKEN }}\"\n";
    let text = workflow(&view(&[root], Some(config))?)?;
    assert!(text.contains("    runs-on: '${{ ''${{'' }} secrets.TOKEN }}'\n"));
    assert!(
        !text.contains("${{ github.event.pull_request.title"),
        "{text}"
    );
    assert!(text.contains(
        "        working-directory: 'a${{ ''${{'' }} github.event.pull_request.title }}'\n"
    ));
    assert!(text.contains("  group: 'ci-${{ github.workflow_ref }}-"));
    Ok(())
}

#[test]
fn an_invalid_config_is_rejected() -> TestResult {
    let cases: [&[u8]; 11] = [
        b"runnr = \"ubuntu-latest\"\n",
        b"exclude = [\"lba\"]\n",
        b"runner = \n",
        b"runner = \"\xff\"\n",
        b"timeout_minutes = 0\n",
        b"timeout_minutes = 361\n",
        b"exclude = [\"../lab\"]\n",
        b"runner = \"macos-13\"\n",
        b"runner = \"macos-14-large\"\n",
        b"runner = \"macos-15-intel\"\n",
        b"runner = \"windows-latest\"\n",
    ];
    for config in cases {
        let result =
            expected_with_notes(&view(&["Cargo.toml"], Some(config))?).map(|(files, _)| files);
        assert!(
            matches!(
                &result,
                Err(GenerateError::Detect(DetectError::Invalid { path, .. })) if *path == CONFIG_PATH
            ),
            "{:?} gave {result:?}",
            String::from_utf8_lossy(config)
        );
    }
    Ok(())
}

#[test]
fn a_repository_without_a_cargo_root_with_members_is_rejected() -> TestResult {
    let empty = expected_with_notes(&view(&[], None)?).map(|(files, _)| files);
    let excluded = expected_with_notes(&view(&["lab/Cargo.toml"], Some(b"exclude = [\"lab\"]\n"))?)
        .map(|(files, _)| files);
    let root_excluded = expected_with_notes(&view(
        &["Cargo.toml"],
        Some(b"exclude = [\"Cargo.toml\"]\n"),
    )?)
    .map(|(files, _)| files);
    let mut memberless = view(&["Cargo.toml"], None)?;
    for metadata in memberless.rust_metadata.values_mut() {
        metadata.members.clear();
    }
    assert_eq!(empty, Err(GenerateError::Detect(DetectError::NoRustRoot)));
    assert_eq!(
        excluded,
        Err(GenerateError::Detect(DetectError::NoRustRoot))
    );
    assert_eq!(
        root_excluded,
        Err(GenerateError::Detect(DetectError::NoRustRoot))
    );
    assert_eq!(
        expected_with_notes(&memberless).map(|(files, _)| files),
        Err(GenerateError::Detect(DetectError::NoRustRoot))
    );
    Ok(())
}
