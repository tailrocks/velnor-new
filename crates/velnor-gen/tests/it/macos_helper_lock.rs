// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, expected_with_notes};
use velnor_model::{RepoPath, RepoView};

use crate::facts::TestResult;
use crate::pipeline::{pin_helper, view};

fn repo(runner: &str, macos_digest: bool) -> Result<RepoView, Box<dyn std::error::Error>> {
    let config = format!("runner = \"{runner}\"\nhosted = false\n");
    let mut repo = view(&["Cargo.toml"], Some(config.as_bytes()))?;
    pin_helper(&mut repo, "0.1.99")?;
    if !macos_digest {
        let lock = RepoPath::new(".velnor/rust-ci.lock")?;
        let text = String::from_utf8(repo.files.remove(&lock).ok_or("no helper lock")?)?;
        let linux_only: String = text
            .lines()
            .filter(|line| !line.starts_with("macos-arm64"))
            .flat_map(|line| [line, "\n"])
            .collect();
        repo.files.insert(lock, linux_only.into_bytes());
    }
    Ok(repo)
}

#[test]
fn a_macos_runner_needs_a_helper_lock_with_a_macos_digest() -> TestResult {
    for runner in ["macos-latest", "macos-15", "MacOS-26"] {
        expected_with_notes(&repo(runner, true)?)?;
        match expected_with_notes(&repo(runner, false)?) {
            Err(GenerateError::HelperLockLacksMacos { runner: named, .. }) => {
                assert_eq!(named, runner);
            }
            other => return Err(format!("{runner}: {:?}", other.map(|_| ())).into()),
        }
    }
    Ok(())
}

#[test]
fn a_linux_only_helper_lock_still_serves_a_linux_runner() -> TestResult {
    expected_with_notes(&repo("ubuntu-latest", false)?)?;
    Ok(())
}

#[test]
fn the_refusal_names_the_fix() -> TestResult {
    let error = expected_with_notes(&repo("macos-latest", false)?)
        .err()
        .ok_or("accepted")?
        .to_string();
    assert!(error.contains("macos-arm64"), "{error}");
    assert!(error.contains("pin-helper"), "{error}");
    Ok(())
}
