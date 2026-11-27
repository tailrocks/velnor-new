// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_model::{RepoPath, RepoView};
use velnor_rust::{CONFIG_PATH, DetectError, Reason, detect};

use super::root::root_and_member;
use super::{FEATURES, MANIFEST_TEXT, refusal, repository};
use crate::facts::TestResult;
use crate::pipeline::workflow;

const OPTIONAL_DEPENDENCY: &str = "[package]\nname = \"package-0\"\n[dependencies]\nserde = { version = \"1\", optional = true }\n";

fn with_manifest(path: &str, manifest: &str, config: &str) -> Result<RepoView, Box<dyn Error>> {
    let mut repo = repository(&["Cargo.toml", "lab/Cargo.toml"], config)?;
    repo.files
        .insert(RepoPath::new(path)?, manifest.as_bytes().to_vec());
    Ok(repo)
}

#[test]
fn a_name_that_a_package_of_every_root_does_not_define_under_features_is_refused() -> TestResult {
    let other = "[package]\nname = \"package-1\"\n[features]\nother = []\n";
    let repo = with_manifest("lab/Cargo.toml", other, FEATURES)?;
    assert_eq!(refusal(&repo)?, ("lab".to_owned(), "local-mode".to_owned()));
    let message = workflow(&repo).err().ok_or("the repository was accepted")?;
    assert!(
        message.to_string().contains(
            "workspace_features names local-mode, which is not a key of the [features] table of any package of the root lab"
        ),
        "{message}"
    );
    let both = with_manifest("lab/Cargo.toml", MANIFEST_TEXT, FEATURES)?;
    detect(&both)?;
    Ok(())
}

#[test]
fn an_optional_dependency_counts_only_when_a_features_entry_names_it() -> TestResult {
    let config = "workspace_features = ['serde']\n";
    let mut repo = repository(&["Cargo.toml"], config)?;
    repo.files.insert(
        RepoPath::new("Cargo.toml")?,
        OPTIONAL_DEPENDENCY.as_bytes().to_vec(),
    );
    assert_eq!(refusal(&repo)?, (".".to_owned(), "serde".to_owned()));
    let named = format!("{OPTIONAL_DEPENDENCY}[features]\nserde = [\"dep:serde\"]\n");
    repo.files
        .insert(RepoPath::new("Cargo.toml")?, named.into_bytes());
    detect(&repo)?;
    Ok(())
}

const GATED: &str =
    "[[test]]\nname = \"gate\"\nharness = false\nrequired-features = [\"local-mode\"]\n";

fn gated_refusal(repo: &RepoView) -> Result<(String, String), Box<dyn Error>> {
    match detect(repo) {
        Err(DetectError::Invalid {
            path,
            reason: Reason::GatedHarnessWithWorkspaceFeatures { package, target },
        }) if path == CONFIG_PATH => Ok((package, target)),
        other => Err(format!("{other:?}").into()),
    }
}

#[test]
fn a_gated_harness_false_target_refuses_the_key_and_nothing_else_does() -> TestResult {
    let manifest = |target: &str| format!("{MANIFEST_TEXT}{target}");
    let mut repo = repository(&["Cargo.toml"], FEATURES)?;
    let gated = manifest(GATED);
    repo.files
        .insert(RepoPath::new("Cargo.toml")?, gated.clone().into_bytes());
    assert_eq!(
        gated_refusal(&repo)?,
        ("package-0".to_owned(), "gate".to_owned())
    );
    let binary = manifest(
        "[[bin]]\nname = \"tool\"\nharness = false\nrequired-features = [\"local-mode\"]\n",
    );
    repo.files
        .insert(RepoPath::new("Cargo.toml")?, binary.into_bytes());
    assert_eq!(
        gated_refusal(&repo)?,
        ("package-0".to_owned(), "tool".to_owned())
    );
    let mut member = root_and_member("[workspace]\nmembers = [\"crates/member\"]\n", FEATURES)?;
    member.files.insert(
        RepoPath::new("crates/member/Cargo.toml")?,
        gated.clone().into_bytes(),
    );
    assert_eq!(
        gated_refusal(&member)?,
        ("member".to_owned(), "gate".to_owned())
    );
    for accepted in [
        "[[test]]\nname = \"gate\"\nharness = false\n",
        "[[test]]\nname = \"gate\"\nharness = false\nrequired-features = []\n",
        "[[test]]\nname = \"gate\"\nrequired-features = [\"local-mode\"]\n",
    ] {
        repo.files.insert(
            RepoPath::new("Cargo.toml")?,
            manifest(accepted).into_bytes(),
        );
        detect(&repo)?;
    }
    let mut without_key = repository(&["Cargo.toml"], "")?;
    without_key
        .files
        .insert(RepoPath::new("Cargo.toml")?, gated.into_bytes());
    detect(&without_key)?;
    Ok(())
}
