// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;

use velnor_gen::expected_files;
use velnor_model::{MisePlatform, RepoPath};

use crate::facts::TestResult;
use crate::pipeline::view;

pub(crate) fn generate(
    filter: &str,
    runner: &str,
    prebuilt: bool,
    deny: bool,
) -> Result<String, String> {
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
            .is_ok_and(|text| text.contains("cargo-deny --config")),
        "{unix:?}"
    );
    let wrong_arch = generate("linux/arm64", "ubuntu-latest", true, true);
    assert!(wrong_arch.is_err_and(|error| error.contains("cargo-deny is not pinned")));
    assert!(generate("linux/arm64", "ubuntu-24.04-arm", true, true)?.contains("cargo-deny --config"));
    Ok(())
}

#[test]
fn self_hosted_labels_preserve_os_filter_matching() -> TestResult {
    let runner = "self-hosted-linux-x64";
    let linux = generate("linux", runner, true, true)?;
    assert!(linux.contains("cargo-deny --config"));
    assert!(
        generate("macos", runner, true, true)
            .is_err_and(|error| error.contains("cargo-deny is not pinned"))
    );
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

#[test]
fn a_prebuilt_is_checked_only_for_the_configured_lock_option_variant() -> TestResult {
    let mut repo = view(&["Cargo.toml"], None)?;
    let mise = |flavor| {
        format!(
            "[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\ntool = {{ version = \"1\", flavor = \"{flavor}\" }}\n"
        )
    };
    let lock = |linux, mac| {
        let mut lock = "[[tools.rust]]\nversion = \"1.99.1\"\n[[tools.\"aqua:nextest-rs/nextest/cargo-nextest\"]]\nversion = \"0.9.151\"\n[tools.\"aqua:nextest-rs/nextest/cargo-nextest\".\"platforms.linux-x64\"]\nurl = \"https://fixtures.invalid/nextest\"\n".to_owned();
        for (flavor, prebuilt) in [("linux", linux), ("mac", mac)] {
            write!(lock, "[[tools.tool]]\nversion = \"1.0.0\"\nspecifiers = [\"1\"]\noptions = {{ flavor = \"{flavor}\" }}\n").unwrap_or_else(|e| unreachable!("{e}"));
            if prebuilt {
                lock.push_str("[tools.tool.\"platforms.linux-x64\"]\nurl = \"https://fixtures.invalid/tool\"\n");
            }
        }
        lock
    };
    let config = RepoPath::from_static("mise.toml");
    let locked = RepoPath::from_static("mise.lock");
    repo.files
        .insert(config.clone(), mise("linux").into_bytes());
    repo.files
        .insert(locked.clone(), lock(true, false).into_bytes());
    let linux = expected_files(&repo);
    assert!(linux.is_ok(), "{linux:?}");
    repo.files.insert(config, mise("mac").into_bytes());
    let mac = expected_files(&repo);
    assert!(mac.is_err_and(|error| error.to_string().contains("prebuilt")));
    repo.files.insert(locked, lock(true, true).into_bytes());
    assert!(expected_files(&repo).is_ok());
    Ok(())
}

#[test]
fn macos_large_and_xlarge_use_their_distinct_runner_architectures() {
    let mut mismatches = Vec::new();
    for (runner, own, other) in [
        ("macos-15-large", "macos/x64", "macos/arm64"),
        ("macos-15-xlarge", "macos/arm64", "macos/x64"),
    ] {
        let platform = MisePlatform::runner(runner);
        if !platform.is_some_and(|platform| platform.matches(own) && !platform.matches(other)) {
            mismatches.push(format!("{runner} platform: {platform:?}"));
        }
        if runner.ends_with("-large") {
            continue;
        }
        let present = generate(own, runner, true, true);
        if !present
            .as_ref()
            .is_ok_and(|text| text.contains("cargo-deny --config"))
        {
            mismatches.push(format!("{runner} applicable policy: {present:?}"));
        }
        let absent = generate(other, runner, true, true);
        if !absent
            .as_ref()
            .is_err_and(|error| error.contains("not pinned"))
        {
            mismatches.push(format!(
                "{runner} foreign policy: {:?}",
                absent.as_ref().map(|_| "workflow generated")
            ));
        }
        let prebuilt = generate(own, runner, false, false);
        if !prebuilt
            .as_ref()
            .is_err_and(|error| error.contains("prebuilt"))
        {
            mismatches.push(format!("{runner} missing prebuilt: {prebuilt:?}"));
        }
    }
    assert!(
        mismatches.is_empty(),
        "wrong macOS architectures: {mismatches:?}"
    );
}

#[test]
fn a_configured_backend_option_cannot_disappear_from_the_lock_identity() -> TestResult {
    let mut repo = view(&["Cargo.toml"], None)?;
    let config = RepoPath::from_static("mise.toml");
    let locked = RepoPath::from_static("mise.lock");
    let original_config = String::from_utf8(repo.files.get(&config).ok_or("no config")?.clone())?;
    let original_lock = String::from_utf8(repo.files.get(&locked).ok_or("no lock")?.clone())?;
    let row = "[[tools.tool]]\nversion = \"1.0.0\"\nspecifiers = [\"1\"]\n[tools.tool.\"platforms.linux-x64\"]\nurl = \"https://fixtures.invalid/tool\"\n";
    repo.files.insert(
        config.clone(),
        format!("{original_config}tool = {{ version = \"1\", flavor = \"linux\" }}\n").into_bytes(),
    );
    repo.files
        .insert(locked.clone(), format!("{original_lock}{row}").into_bytes());
    let missing = expected_files(&repo);
    assert!(
        missing.is_err(),
        "missing option identity: {:?}",
        missing.as_ref().map(|_| "workflow generated")
    );
    let recorded = row.replace(
        "specifiers = [\"1\"]",
        "specifiers = [\"1\"]\noptions = { flavor = \"linux\" }",
    );
    repo.files.insert(
        locked.clone(),
        format!("{original_lock}{recorded}").into_bytes(),
    );
    assert!(expected_files(&repo).is_ok());
    repo.files.insert(
        config,
        format!(
            "{original_config}tool = {{ version = \"1\", lazy = true, depends = [\"rust\"] }}\n"
        )
        .into_bytes(),
    );
    repo.files
        .insert(locked, format!("{original_lock}{row}").into_bytes());
    assert!(expected_files(&repo).is_ok());
    Ok(())
}
