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
    for (label, repository) in cases {
        let written = decided(
            label,
            &format!("{DATABASE}/{repository}"),
            "hooks/post-fetch",
            &format!("TOKEN={SHA}\n"),
        );
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
