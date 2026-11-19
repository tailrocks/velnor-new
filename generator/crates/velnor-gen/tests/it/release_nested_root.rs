// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use velnor_gen::{GenerateError, expected_files, release_ruleset};
use velnor_release::ReleaseError;
use velnor_rust::LOCK_PATH;

use crate::release_fixture::{APP, LOCK, Root, TOOLS, release_view};

type TestResult = Result<(), Box<dyn Error>>;

const TWO_ROOTS: &str = "roots = [\".\", \"tools\"]\n";

fn view(roots: &[Root], release_toml: &str) -> Result<velnor_model::RepoView, Box<dyn Error>> {
    let mut view = release_view(roots, Some(release_toml))?;
    view.files.insert(LOCK_PATH, LOCK.as_bytes().to_vec());
    Ok(view)
}

fn names_the_root(error: &GenerateError, root: &str) -> bool {
    matches!(error, GenerateError::Release(ReleaseError::NestedReleasePlzRoot { root: found }) if found == root)
        && error.to_string().contains(root)
}

#[test]
fn a_release_root_below_the_git_root_is_refused_whatever_it_publishes() -> TestResult {
    let publishing = Root {
        publishes: true,
        ..TOOLS
    };
    for tools in [TOOLS, publishing] {
        let error = expected_files(&view(&[APP, tools], TWO_ROOTS)?)
            .err()
            .ok_or("accepted a nested release root")?;
        assert!(names_the_root(&error, "tools"), "{error}");
    }
    Ok(())
}

#[test]
fn the_release_ruleset_refuses_a_release_root_below_the_git_root() -> TestResult {
    let error = release_ruleset(&view(&[APP, TOOLS], TWO_ROOTS)?)
        .err()
        .ok_or("printed a ruleset for a nested release root")?;
    assert!(names_the_root(&error, "tools"), "{error}");
    Ok(())
}

#[test]
fn a_release_root_at_the_git_root_is_generated() -> TestResult {
    assert!(expected_files(&view(&[APP], "roots = [\".\"]\n")?).is_ok());
    Ok(())
}
