// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoPath;

use super::events::DENY_MISE;
use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

fn policy_of(
    config: Option<&str>,
    deny_files: &[&str],
) -> Result<String, Box<dyn std::error::Error>> {
    let mut repo = view(
        &["Cargo.toml", "tools/Cargo.toml"],
        config.map(str::as_bytes),
    )?;
    for file in deny_files {
        let path = RepoPath::new(*file)?;
        repo.tracked_paths.insert(path.clone());
        repo.files.insert(path, Vec::new());
    }
    repo.files
        .insert(RepoPath::new("mise.toml")?, DENY_MISE.as_bytes().to_vec());
    let text = workflow(&repo)?;
    Ok(text
        .split("\n  policy:\n")
        .nth(1)
        .ok_or("no policy job")?
        .split("\n  required:\n")
        .next()
        .ok_or("no policy job end")?
        .to_owned())
}

#[test]
fn deny_runs_locked_with_each_roots_own_config_when_configured() -> TestResult {
    let policy = policy_of(
        Some("[deny]\nlocked = true\nconfig = 'deny.toml'\n"),
        &["deny.toml", "tools/deny.toml"],
    )?;
    assert!(
        policy.contains("cargo-deny --locked --config ''deny.toml'' check"),
        "{policy}"
    );
    assert!(
        policy.contains(
            "cargo-deny --locked --manifest-path ''tools/Cargo.toml'' --config ''tools/deny.toml'' check"
        ),
        "{policy}"
    );
    Ok(())
}

#[test]
fn deny_without_the_deny_table_keeps_its_plain_command() -> TestResult {
    let policy = policy_of(None, &["deny.toml"])?;
    assert!(policy.contains("cargo-deny check"), "{policy}");
    assert!(
        !policy.contains("--locked") && !policy.contains("--config"),
        "{policy}"
    );
    Ok(())
}

#[test]
fn a_deny_config_that_is_not_a_tracked_file_of_every_root_or_not_a_relative_path_is_rejected()
-> TestResult {
    for (config, files, mention) in [
        ("[deny]\nconfig = 'deny.toml'\n", vec!["deny.toml"], "tools"),
        (
            "[deny]\nconfig = '../deny.toml'\n",
            vec!["deny.toml", "tools/deny.toml"],
            "deny.config",
        ),
        (
            "[deny]\nconfig = '/etc/deny.toml'\n",
            vec!["deny.toml", "tools/deny.toml"],
            "deny.config",
        ),
        (
            "[deny]\nconfig = ''\n",
            vec!["deny.toml", "tools/deny.toml"],
            "deny.config",
        ),
        ("[deny]\nlocked = 'yes'\n", vec![], "boolean"),
        ("[deny]\nfrozen = true\n", vec![], "frozen"),
    ] {
        let error = policy_of(Some(config), &files)
            .err()
            .ok_or_else(|| format!("accepted: {config}"))?;
        assert!(error.to_string().contains(mention), "{config}: {error}");
    }
    Ok(())
}
