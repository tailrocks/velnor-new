// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;

use velnor_gen::expected_files;
use velnor_model::RepoPath;

use crate::facts::TestResult;
use crate::pipeline::view;

fn generate(filter: &str, runner: &str, prebuilt: bool, deny: bool) -> Result<String, String> {
    let config = format!("runner = \"{runner}\"\n");
    let mut repo = view(&["Cargo.toml"], Some(config.as_bytes())).map_err(|e| e.to_string())?;
    let mise = format!(
        "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\ncargo-deny = {{ version = \"0.20.2\", os = [\"{filter}\"] }}\n"
    );
    let mut lock = "[[tools.rust]]\nversion = \"1.99.1\"\n\n[[tools.\"aqua:nextest-rs/nextest/cargo-nextest\"]]\nversion = \"0.9.151\"\n".to_owned();
    for platform in ["linux-x64", "linux-arm64", "macos-arm64", "macos-x64"] {
        write!(lock, "[tools.\"aqua:nextest-rs/nextest/cargo-nextest\".\"platforms.{platform}\"]\nurl = \"https://fixtures.invalid/nextest\"\n").map_err(|error| error.to_string())?;
    }
    lock.push_str("[[tools.cargo-deny]]\nversion = \"0.20.2\"\nspecifiers = [\"0.20.2\"]\n");
    if prebuilt {
        for platform in ["linux-x64", "linux-arm64", "macos-arm64", "macos-x64"] {
            write!(lock, "[tools.cargo-deny.\"platforms.{platform}\"]\nurl = \"https://fixtures.invalid/deny\"\n").map_err(|error| error.to_string())?;
        }
    }
    for (path, text) in [("mise.toml", mise), ("mise.lock", lock)] {
        let path = RepoPath::new(path).map_err(|e| e.to_string())?;
        repo.tracked_paths.insert(path.clone());
        repo.files.insert(path, text.into_bytes());
    }
    if deny {
        let path = RepoPath::from_static("deny.toml");
        repo.tracked_paths.insert(path.clone());
        repo.files.insert(path, Vec::new());
    }
    expected_files(&repo)
        .map(|files| files.into_values().collect::<Vec<_>>().join("\n"))
        .map_err(|e| e.to_string())
}

#[test]
fn generator_policy_tools_follow_mise_os_and_architecture_filters() -> TestResult {
    let unix = generate("unix", "ubuntu-latest", true, true);
    assert!(
        unix.as_ref()
            .is_ok_and(|text| text.contains("cargo-deny check")),
        "{unix:?}"
    );
    let wrong_arch = generate("linux/arm64", "ubuntu-latest", true, true);
    assert!(wrong_arch.is_err_and(|error| error.contains("cargo-deny is not pinned")));
    assert!(generate("linux/arm64", "ubuntu-24.04-arm", true, true)?.contains("cargo-deny check"));
    Ok(())
}

#[test]
fn an_applicable_mise_filter_cannot_skip_the_missing_prebuilt_error() {
    let cases = [
        ("linux/x64", "ubuntu-latest"),
        ("unix", "ubuntu-latest"),
        ("darwin", "macos-latest"),
    ];
    let mismatches: Vec<_> = cases
        .into_iter()
        .filter(|(filter, runner)| {
            !generate(filter, runner, false, false)
                .is_err_and(|error| error.contains("cargo-deny") && error.contains("prebuilt"))
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "skipped prebuilt checks: {mismatches:?}"
    );
    assert!(generate("linux/arm64", "ubuntu-latest", false, false).is_ok());
}

#[test]
fn every_applicable_locked_version_needs_its_own_prebuilt_for_the_runner() -> TestResult {
    let mut repo = view(&["Cargo.toml"], None)?;
    repo.files.insert(RepoPath::from_static("mise.toml"), b"[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\ntool = [\"1\", \"2\"]\n".to_vec());
    let mut lock = "[[tools.rust]]\nversion = \"1.99.1\"\n[[tools.\"aqua:nextest-rs/nextest/cargo-nextest\"]]\nversion = \"0.9.151\"\n[tools.\"aqua:nextest-rs/nextest/cargo-nextest\".\"platforms.linux-x64\"]\nurl = \"https://fixtures.invalid/nextest\"\n[[tools.tool]]\nversion = \"1.0.0\"\nspecifiers = [\"1\"]\n[tools.tool.\"platforms.linux-x64\"]\nurl = \"https://fixtures.invalid/tool-1\"\n[[tools.tool]]\nversion = \"2.0.0\"\nspecifiers = [\"2\"]\n".to_owned();
    let path = RepoPath::from_static("mise.lock");
    repo.files.insert(path.clone(), lock.as_bytes().to_vec());
    let error = expected_files(&repo)
        .err()
        .ok_or("second row must fail prebuilt validation")?;
    assert!(
        error.to_string().contains("tool") && error.to_string().contains("prebuilt"),
        "{error}"
    );
    lock.push_str(
        "[tools.tool.\"platforms.linux-x64\"]\nurl = \"https://fixtures.invalid/tool-2\"\n",
    );
    repo.files.insert(path, lock.into_bytes());
    assert!(expected_files(&repo).is_ok());
    Ok(())
}
