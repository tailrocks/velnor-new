// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache_private_repo::{GIT_LOCK, decide_with, private_checkout};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const DATABASE: &str = ".cargo/git/db/shared-1";

fn decided(label: &str, repository: &str, file: &str, content: &str) -> String {
    let repo = private_checkout(&format!("decide-modules-{label}"), GIT_LOCK);
    repo.write_unstaged(&format!("home/{repository}/{file}"), content);
    let (output, written) = decide_with(&repo, Some("true"));
    assert!(output.status.success(), "{label}: {output:?}");
    written
}

fn hook_stops(cases: &[(&str, &str)]) {
    hook_stops_beside(cases, "HEAD", "ref: refs/heads/main\n");
}

fn hook_stops_beside(cases: &[(&str, &str)], marker: &str, content: &str) {
    for (label, repository) in cases {
        let repo = private_checkout(&format!("decide-modules-marker-{label}"), GIT_LOCK);
        repo.write_unstaged(&format!("home/{DATABASE}/{repository}/{marker}"), content);
        repo.write_unstaged(
            &format!("home/{DATABASE}/{repository}/hooks/post-fetch"),
            &format!("TOKEN={SHA}\n"),
        );
        let (output, written) = decide_with(&repo, Some("true"));
        assert!(output.status.success(), "{label}: {output:?}");
        assert_eq!(written, "save=true\nsources=false\n", "{label}");
    }
}

fn ref_goes_on(cases: &[(&str, &str, &str)]) {
    for (label, repository, file) in cases {
        let written = decided(label, &format!("{DATABASE}/{repository}"), file, SHA);
        assert_eq!(written, "save=true\nsources=true\n", "{label}");
    }
}

#[test]
fn the_hook_tree_of_a_submodule_named_refs_is_scanned() {
    hook_stops(&[
        ("module-named-refs", "modules/refs"),
        ("module-path-ending-in-refs", "modules/vendor/refs"),
        ("module-path-through-refs", "modules/refs/vendor"),
        ("module-path-ending-in-two-refs", "modules/refs/refs"),
        (
            "module-named-refs-in-a-nested-module",
            "modules/vendor/modules/refs",
        ),
    ]);
}

#[test]
fn the_hook_tree_of_a_submodule_named_objects_is_scanned() {
    hook_stops(&[
        ("module-named-objects", "modules/objects"),
        ("module-path-ending-in-objects", "modules/vendor/objects"),
        ("module-path-through-objects", "modules/objects/vendor"),
    ]);
}

#[test]
fn the_hook_tree_of_a_nested_submodule_is_scanned() {
    hook_stops(&[
        ("nested-module", "modules/vendor/modules/deep"),
        (
            "nested-module-twice",
            "modules/vendor/modules/deep/modules/deeper",
        ),
        (
            "nested-module-in-a-refs-module",
            "modules/vendor/refs/modules/deep",
        ),
        (
            "nested-module-named-refs-in-a-refs-module",
            "modules/refs/modules/refs",
        ),
    ]);
}

#[test]
fn the_hook_tree_below_any_directory_of_the_modules_tree_is_scanned() {
    hook_stops(&[
        ("hooks-directly-in-modules-refs", "modules/refs"),
        ("segment-after-refs", "modules/vendor/refs/x"),
        ("segments-after-refs", "modules/vendor/refs/x/y/z"),
        ("segment-after-objects", "modules/objects/x"),
        (
            "segment-after-objects-in-a-module",
            "modules/vendor/objects/x",
        ),
        ("three-level-nested-module", "modules/a/modules/b/modules/c"),
        (
            "three-level-nested-module-below-refs",
            "modules/a/refs/modules/b/refs/x/modules/c",
        ),
    ]);
}

#[test]
fn a_config_file_marks_a_git_directory_as_well_as_a_head_file() {
    hook_stops_beside(
        &[
            ("config-marker", "modules/vendor/refs/x"),
            ("config-marker-nested", "modules/a/modules/b/modules/c"),
        ],
        "config",
        "[core]\n\tbare = true\n",
    );
}

#[test]
fn the_modules_of_a_linked_worktree_are_scanned() {
    hook_stops(&[
        ("linked-worktree-module", "worktrees/w1/modules/vendor"),
        (
            "linked-worktree-nested-module",
            "worktrees/w1/modules/vendor/modules/deep",
        ),
    ]);
}

#[test]
fn a_hooks_directory_without_a_git_directory_parent_is_not_a_hook_tree() {
    for (label, directory) in [
        ("plain-directory", "modules/vendor/plain"),
        (
            "plain-directory-below-refs",
            "modules/vendor/refs/heads/plain",
        ),
        ("plain-directory-below-objects", "modules/vendor/objects/ab"),
    ] {
        let written = decided(label, &format!("{DATABASE}/{directory}"), "hooks/main", SHA);
        assert_eq!(written, "save=true\nsources=true\n", "{label}");
    }
}

#[test]
fn refs_of_a_submodule_repository_are_not_the_hook_tree_and_are_saved() {
    ref_goes_on(&[
        ("heads-hooks", "modules/vendor", "refs/heads/hooks/main"),
        (
            "remotes-hooks",
            "modules/vendor",
            "refs/remotes/origin/hooks/main",
        ),
        (
            "heads-hooks-in-a-refs-module",
            "modules/vendor/refs",
            "refs/heads/hooks/main",
        ),
        (
            "heads-hooks-in-a-module-named-refs",
            "modules/refs",
            "refs/heads/hooks/main",
        ),
        (
            "heads-hooks-in-a-nested-module",
            "modules/vendor/modules/deep",
            "refs/heads/hooks/main",
        ),
        (
            "tags-hooks-in-a-refs-module",
            "modules/vendor/refs",
            "refs/tags/hooks/v1",
        ),
    ]);
}

#[test]
fn a_remote_head_symref_does_not_make_a_refs_directory_a_hook_tree() {
    let repo = private_checkout("decide-modules-remote-head", GIT_LOCK);
    repo.write_unstaged(
        &format!("home/{DATABASE}/modules/vendor/HEAD"),
        "ref: refs/heads/main\n",
    );
    repo.write_unstaged(
        &format!("home/{DATABASE}/modules/vendor/refs/remotes/origin/HEAD"),
        "ref: refs/remotes/origin/main\n",
    );
    repo.write_unstaged(
        &format!("home/{DATABASE}/modules/vendor/refs/remotes/origin/hooks/main"),
        SHA,
    );
    let (output, written) = decide_with(&repo, Some("true"));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=true\n", "{output:?}");
}
