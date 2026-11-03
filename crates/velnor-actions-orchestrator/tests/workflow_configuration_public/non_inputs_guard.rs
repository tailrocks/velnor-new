// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::process::Output;

use super::non_inputs_checkout_support::{
    apply_exclusion, git_output, guard_command, guard_run, rendered_patterns, tracked_tree,
};
use super::scoped_workspace::TestResult;
use super::workflow_configuration_fixture::git;

const GLOBS: [(&str, &[&str], &[&str]); 2] = [
    (
        "docs/**",
        &["docs/a.md", "docs/guide.md", "docs/sub/b.md"],
        &["docsx/c.md", "apps/a1/docs/d.md", "crates/x/src/lib.rs"],
    ),
    (
        "docs/*/**",
        &["docs/sub/b.md"],
        &[
            "docs/a.md",
            "docs/guide.md",
            "docsx/c.md",
            "apps/a1/docs/d.md",
            "crates/x/src/lib.rs",
        ],
    ),
];

#[test]
fn rendered_exclusion_removes_non_inputs_on_a_real_git_tree() -> TestResult {
    for (glob, gone, kept) in GLOBS {
        let tree = tracked_tree()?;
        let root = tree.path();
        apply_exclusion(root, &rendered_patterns(glob)?)?;
        for path in gone {
            assert!(!root.join(path).exists(), "{glob}: {path} survived");
        }
        for path in kept {
            assert!(root.join(path).exists(), "{glob}: {path} was over-matched");
        }
        assert_eq!(git_output(root, &["status", "--porcelain"])?, "", "{glob}");
    }
    Ok(())
}

#[test]
fn absence_guard_fails_when_a_non_input_is_present() -> TestResult {
    for (glob, gone, kept) in GLOBS {
        let tree = tracked_tree()?;
        let root = tree.path();
        let present = guard_run(glob, root)?;
        assert!(
            !present.status.success(),
            "{glob}: guard passed with the non-input present"
        );
        let stderr = String::from_utf8(present.stderr)?;
        let first = gone.first().ok_or("no non-input path in the case")?;
        assert!(
            stderr.contains(&format!("non_input_present:{first}")),
            "{glob}: {first} not named in {stderr}"
        );
        for path in kept {
            assert!(
                !stderr.contains(path),
                "{glob}: kept {path} named in {stderr}"
            );
        }
        apply_exclusion(root, &rendered_patterns(glob)?)?;
        let absent = guard_run(glob, root)?;
        assert!(
            absent.status.success(),
            "{glob}: {}",
            String::from_utf8_lossy(&absent.stderr)
        );
        let only_lookalikes = tempfile::tempdir()?;
        let lookalikes = only_lookalikes.path();
        git(&["init", "-b", "testmain"], lookalikes)?;
        fs::create_dir_all(lookalikes.join("docsx"))?;
        fs::write(lookalikes.join("docsx/c.md"), "c\n")?;
        git(&["add", "-A"], lookalikes)?;
        git(&["commit", "-m", "lookalike"], lookalikes)?;
        assert!(guard_run(glob, lookalikes)?.status.success(), "{glob}");
    }
    Ok(())
}

fn assert_loud_failure(label: &str, output: &Output) {
    assert!(
        !output.status.success(),
        "{label}: guard passed although git could not list files"
    );
    assert!(
        !output.stderr.is_empty(),
        "{label}: guard failed without a message"
    );
}

#[test]
fn absence_guard_fails_loudly_when_git_cannot_list_files() -> TestResult {
    let glob = "docs/**";
    let outside = tempfile::tempdir()?;
    let parent = outside.path().parent().ok_or("tempdir has no parent")?;
    let not_a_repository = guard_command(glob)?
        .current_dir(outside.path())
        .env("GIT_CEILING_DIRECTORIES", parent)
        .output()?;
    assert_loud_failure("not a repository", &not_a_repository);

    let tree = tracked_tree()?;
    let root = tree.path();
    apply_exclusion(root, &rendered_patterns(glob)?)?;
    let healthy = guard_run(glob, root)?;
    assert!(
        healthy.status.success(),
        "control: {}",
        String::from_utf8_lossy(&healthy.stderr)
    );
    fs::write(root.join(".git/index"), "not an index\n")?;
    assert_loud_failure("corrupt index", &guard_run(glob, root)?);
    Ok(())
}
