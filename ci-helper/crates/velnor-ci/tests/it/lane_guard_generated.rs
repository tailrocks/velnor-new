// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use clap::Parser;
use velnor_ci::cli::{Cli, Command};
use velnor_gen::expected_files;
use velnor_model::{
    PackageName, RepoPath, RepoView, RustMetadata, RustPackage, Target, TargetKind, TargetName,
};
use velnor_rust::{CONFIG_PATH, LOCK_PATH};

type TestResult = Result<(), Box<dyn Error>>;

const HELPER_LOCK: &str =
    include_str!("../../../../../generator/samples/helper-lock/view/.velnor/rust-ci.lock");
const WORKFLOW: &str = ".github/workflows/rust-ci.yml";
const LAUNCHER: &str = ". \"$VELNOR_EXEC\" run -- ";
const MISE: &str =
    "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\n";
const MISE_LOCK: &str = "[[tools.\"aqua:nextest-rs/nextest/cargo-nextest\"]]\nversion = \"1\"\n\n[tools.\"aqua:nextest-rs/nextest/cargo-nextest\".\"platforms.linux-x64\"]\nurl = \"https://example.invalid/nextest\"\n\n[tools.\"aqua:nextest-rs/nextest/cargo-nextest\".\"platforms.macos-arm64\"]\nurl = \"https://example.invalid/nextest\"\n\n[[tools.rust]]\nversion = \"1\"\n\n[tools.rust.\"platforms.linux-x64\"]\nurl = \"https://example.invalid/rust\"\n\n[tools.rust.\"platforms.macos-arm64\"]\nurl = \"https://example.invalid/rust\"\n";
const LANE: &str = "[[test_lane]]\nroot = '.'\nfeatures = ['local-mode', 'extra']\nfilter = \"test(~local::) or binary(it's)\"\n";

fn view(helper: &str, config: &str) -> Result<RepoView, Box<dyn Error>> {
    let mut view = RepoView::default();
    let manifest = RepoPath::new("Cargo.toml")?;
    view.tracked_paths
        .insert(manifest.sibling(&RepoPath::new("Cargo.lock")?));
    view.tracked_paths.insert(manifest.clone());
    let member = RustPackage {
        name: PackageName::new("package")?,
        version: Some("0.1.0".to_owned()),
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
    for (path, text) in [
        (RepoPath::new("mise.toml")?, MISE.to_owned()),
        (RepoPath::new("mise.lock")?, MISE_LOCK.to_owned()),
        (LOCK_PATH, HELPER_LOCK.replace("0.1.79", helper)),
        (CONFIG_PATH, config.to_owned()),
    ] {
        view.tracked_paths.insert(path.clone());
        view.files.insert(path, text.into_bytes());
    }
    Ok(view)
}

fn shell_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut quoted = false;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                quoted = !quoted;
                started = true;
            }
            '\\' if !quoted => {
                word.extend(chars.next());
                started = true;
            }
            c if c.is_whitespace() && !quoted => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            c => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    words
}

#[test]
fn the_generated_step_parses_with_this_cli() -> TestResult {
    let files = expected_files(&view("0.1.82", LANE)?)?;
    let text = files
        .get(&RepoPath::new(WORKFLOW)?)
        .ok_or("the workflow was not rendered")?;
    let lines: Vec<&str> = text
        .lines()
        .filter(|line| line.contains("velnor-ci lane-guard"))
        .collect();
    assert_eq!(lines.len(), 1, "{lines:?}");
    let run = lines[0]
        .trim()
        .strip_prefix("run: '")
        .and_then(|rest| rest.strip_suffix('\''))
        .ok_or(lines[0])?
        .replace("''", "'");
    let command = run.strip_prefix(LAUNCHER).ok_or(run.as_str())?;
    let cli = Cli::try_parse_from(shell_words(command))?;
    let Command::LaneGuard(args) = cli.command else {
        return Err(run.into());
    };
    assert_eq!(args.features, "local-mode,extra");
    assert_eq!(args.filter, "test(~local::) or binary(it's)");
    assert_eq!(args.except_binary.len(), 0);
    Ok(())
}
