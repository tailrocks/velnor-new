// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write;

use velnor_gen::expected_with_notes;
use velnor_model::{RepoPath, RepoView};

use crate::base_save::pinned;
use crate::facts::TestResult;

const MISMATCH: &str = "[private_sources].repositories";

fn locked(
    sources: &[(&str, &str)],
    listed: &[&str],
) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = pinned("0.1.88", "")?;
    let mut lock =
        "version = 4\n\n[[package]]\nname = \"package-0\"\nversion = \"0.1.0\"\n".to_owned();
    for (name, source) in sources {
        write!(
            lock,
            "\n[[package]]\nname = \"{name}\"\nversion = \"1.0.0\"\nsource = \"{source}\"\n"
        )?;
    }
    view.files
        .insert(RepoPath::new("Cargo.lock")?, lock.into_bytes());
    let repositories: Vec<String> = listed.iter().map(|name| format!("'acme/{name}'")).collect();
    view.files.insert(
        RepoPath::new(".velnor/rust-ci.toml")?,
        format!(
            "[private_sources]\napp_id = 'vars.A'\nprivate_key = 'secrets.K'\nrepositories = [{}]\n",
            repositories.join(", ")
        )
        .into_bytes(),
    );
    Ok(view)
}

fn mismatches(view: &RepoView) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    Ok(expected_with_notes(view)?
        .1
        .into_iter()
        .filter(|note| note.contains(MISMATCH))
        .collect())
}

#[test]
fn a_locked_repository_of_the_token_owner_that_the_list_misses_is_reported() -> TestResult {
    let view = locked(
        &[
            (
                "shared",
                "git+https://github.com/acme/shared-lib?branch=main#abc",
            ),
            ("other", "git+ssh://git@github.com/acme/other-lib.git#abc"),
        ],
        &["shared-lib"],
    )?;
    let found = mismatches(&view)?;
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("does not list other (acme/other-lib in Cargo.lock)"));
    assert!(!found[0].contains("shared"), "{found:?}");
    Ok(())
}

#[test]
fn a_listed_repository_that_no_lock_uses_is_reported_as_a_wider_token() -> TestResult {
    let view = locked(
        &[("shared", "git+https://github.com/acme/shared-lib#abc")],
        &["shared-lib", "stale-lib"],
    )?;
    let found = mismatches(&view)?;
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("lists acme/stale-lib"), "{found:?}");
    assert!(!found[0].contains("does not list"), "{found:?}");
    Ok(())
}

#[test]
fn a_list_that_matches_the_lock_and_foreign_owners_are_silent() -> TestResult {
    let view = locked(
        &[
            ("shared", "git+https://github.com/Acme/Shared-Lib.git#abc"),
            ("serde", "git+https://github.com/serde-rs/serde#abc"),
            (
                "registry",
                "registry+https://registry.example.invalid/index",
            ),
        ],
        &["shared-lib"],
    )?;
    assert_eq!(mismatches(&view)?, Vec::<String>::new());
    Ok(())
}
