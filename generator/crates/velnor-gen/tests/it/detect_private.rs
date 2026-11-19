// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::expected_with_notes;
use velnor_model::{RepoPath, RepoView, RepositoryIdentity};

use crate::base_save::pinned;
use crate::facts::TestResult;

const SUGGESTION: &str = "add a [private_sources] table to .velnor/rust-ci.toml";

fn with_source(source: &str, owner: Option<&str>) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = pinned("0.1.86", "")?;
    if let Some(name) = owner {
        view.repository = RepositoryIdentity::Named(name.to_owned());
    }
    let lock = format!(
        "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n\n[[package]]\nname = \"dep\"\nversion = \"1.0.0\"\nsource = \"{source}\"\n"
    );
    view.files
        .insert(RepoPath::new("Cargo.lock")?, lock.into_bytes());
    Ok(view)
}

fn suggestions(view: &RepoView) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    Ok(expected_with_notes(view)?
        .1
        .into_iter()
        .filter(|note| note.contains(SUGGESTION))
        .collect())
}

#[test]
fn private_registries_and_private_git_hosts_suggest_the_section() -> TestResult {
    let cases = [
        ("registry+https://registry.example.invalid/index", None),
        ("sparse+https://registry.example.invalid/index/", None),
        ("git+https://git.example.invalid/acme/lib?rev=1#abc", None),
        ("git+ssh://git@github.com/other/lib#abc", None),
        (
            "git+https://github.com/acme/shared-lib?branch=main#abc",
            Some("acme/app"),
        ),
    ];
    for (source, owner) in cases {
        let found = suggestions(&with_source(source, owner)?)?;
        assert_eq!(found.len(), 1, "{source}: {found:?}");
        assert!(found[0].contains("dep"), "{found:?}");
        assert!(found[0].contains("Cargo.lock"), "{found:?}");
    }
    Ok(())
}

#[test]
fn public_dependencies_and_configured_repositories_suggest_nothing() -> TestResult {
    let public = [
        (
            "registry+https://github.com/rust-lang/crates.io-index",
            None,
        ),
        ("sparse+https://index.crates.io/", None),
        (
            "git+https://github.com/serde-rs/serde?branch=main#abc",
            Some("acme/app"),
        ),
        ("git+https://gitlab.com/some/lib#abc", None),
    ];
    for (source, owner) in public {
        let found = suggestions(&with_source(source, owner)?)?;
        assert!(found.is_empty(), "{source}: {found:?}");
    }
    Ok(())
}

#[test]
fn the_suggestion_names_the_key_names_and_holds_no_secret() -> TestResult {
    let view = with_source("registry+https://registry.example.invalid/index", None)?;
    let found = suggestions(&view)?;
    let note = found.first().ok_or("no suggestion")?;
    for part in ["app_id", "private_key", "repositories", "vars.", "secrets."] {
        assert!(note.contains(part), "{part}: {note}");
    }
    assert!(!note.contains("BEGIN"), "{note}");
    Ok(())
}

#[test]
fn a_repository_with_the_section_is_not_told_to_add_it() -> TestResult {
    let mut view = with_source("registry+https://registry.example.invalid/index", None)?;
    view.files.insert(
        RepoPath::new(".velnor/rust-ci.toml")?,
        b"[private_sources]\napp_id = 'vars.A'\nprivate_key = 'secrets.K'\nrepositories = ['acme/lib']\n".to_vec(),
    );
    assert_eq!(suggestions(&view)?, Vec::<String>::new());
    Ok(())
}
