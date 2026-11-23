// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache_private_repo::{GIT_LOCK, decide_with, private_checkout};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const DATABASE: &str = ".cargo/git/db/shared-1";
const BARE: &str = "[core]\n\tbare = true\n";
const TOKEN_REMOTE: &str = concat!(
    "[remote \"origin\"]\n\turl = https://x-access-token:",
    "ghs_",
    "secret@github.com/acme/shared-lib\n"
);

fn decided(label: &str, files: &[(String, String)]) -> (String, crate::io::Repo) {
    let repo = private_checkout(&format!("decide-modules-{label}"), GIT_LOCK);
    for (path, content) in files {
        repo.write_unstaged(&format!("home/{DATABASE}/{path}"), content);
    }
    let (output, written) = decide_with(&repo, Some("true"));
    assert!(output.status.success(), "{label}: {output:?}");
    (written, repo)
}

fn repository(root: &str, config: &str) -> Vec<(String, String)> {
    vec![
        (format!("{root}/HEAD"), "ref: refs/heads/main\n".to_owned()),
        (format!("{root}/config"), config.to_owned()),
    ]
}

#[test]
fn a_credential_in_the_config_of_any_nested_repository_stops_the_sources_save() {
    for (label, root) in [
        ("module-named-refs", "modules/refs"),
        ("module-path-ending-in-refs", "modules/vendor/refs"),
        ("module-path-through-refs", "modules/refs/vendor"),
        ("module-named-objects", "modules/objects"),
        ("module-path-through-objects", "modules/objects/vendor"),
        ("nested-module", "modules/vendor/modules/deep"),
        ("segment-after-refs", "modules/vendor/refs/x"),
        ("segments-after-refs", "modules/vendor/refs/x/y/z"),
        ("segment-after-objects", "modules/objects/x"),
        ("three-level-nested-module", "modules/a/modules/b/modules/c"),
        ("linked-worktree-module", "worktrees/w1/modules/vendor"),
        ("nested-dot-git", "vendor/.git"),
    ] {
        let (written, _repo) = decided(label, &repository(root, TOKEN_REMOTE));
        assert_eq!(written, "save=true\nsources=false\n", "{label}");
    }
}

#[test]
fn the_hooks_of_any_nested_repository_are_left_out_and_the_rest_is_saved() {
    for (label, root) in [
        ("module-named-refs", "modules/refs"),
        ("module-named-objects", "modules/objects"),
        ("segment-after-refs", "modules/vendor/refs/x"),
        ("three-level-nested-module", "modules/a/modules/b/modules/c"),
        ("linked-worktree-module", "worktrees/w1/modules/vendor"),
    ] {
        let mut files = repository(root, BARE);
        files.push((
            format!("{root}/hooks/post-fetch"),
            concat!("TOKEN=", "1\n").to_owned(),
        ));
        let (written, repo) = decided(label, &files);
        assert_eq!(written, "save=true\nsources=true\n", "{label}");
        let home = repo.root().join("home").join(DATABASE).join(root);
        assert!(!home.join("hooks").exists(), "{label}");
        assert!(home.join("HEAD").is_file(), "{label}");
    }
}

#[test]
fn a_directory_with_a_head_but_neither_config_nor_objects_is_not_a_repository() {
    let files = [
        ("modules/vendor/refs/heads/topic/HEAD", SHA),
        ("modules/vendor/refs/heads/topic/hooks/main", SHA),
        (
            "modules/vendor/refs/remotes/origin/HEAD",
            "ref: refs/remotes/origin/main\n",
        ),
        ("modules/vendor/refs/remotes/origin/hooks/main", SHA),
    ]
    .map(|(path, content)| (path.to_owned(), content.to_owned()));
    let mut all = repository("modules/vendor", BARE);
    all.extend(files);
    let (written, repo) = decided("head-without-config", &all);
    assert_eq!(written, "save=true\nsources=true\n");
    let vendor = repo
        .root()
        .join("home")
        .join(DATABASE)
        .join("modules/vendor");
    assert!(vendor.join("refs/heads/topic/hooks/main").is_file());
    assert!(vendor.join("refs/remotes/origin/hooks/main").is_file());
}
