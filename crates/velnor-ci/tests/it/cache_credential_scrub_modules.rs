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

fn nested_configs_stop_the_sources_save(roots: &[(&str, &str)]) {
    for (label, root) in roots {
        let (written, _repo) = decided(label, &repository(root, TOKEN_REMOTE));
        assert_eq!(written, "save=true\nsources=false\n", "{label}");
    }
}

#[test]
fn a_credential_in_the_config_of_a_module_named_like_a_git_directory_stops_the_sources_save() {
    nested_configs_stop_the_sources_save(&[
        ("module-named-refs", "modules/refs"),
        ("module-path-ending-in-refs", "modules/vendor/refs"),
        ("module-path-through-refs", "modules/refs/vendor"),
        ("module-named-objects", "modules/objects"),
        ("module-path-through-objects", "modules/objects/vendor"),
        ("segment-after-refs", "modules/vendor/refs/x"),
    ]);
}

#[test]
fn a_credential_in_the_config_of_any_nested_repository_stops_the_sources_save() {
    nested_configs_stop_the_sources_save(&[
        ("nested-module", "modules/vendor/modules/deep"),
        ("segments-after-refs", "modules/vendor/refs/x/y/z"),
        ("segment-after-objects", "modules/objects/x"),
        ("three-level-nested-module", "modules/a/modules/b/modules/c"),
        ("linked-worktree-module", "worktrees/w1/modules/vendor"),
        ("nested-dot-git", "vendor/.git"),
    ]);
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

const SYMBOLIC_TARGET: &str = concat!(
    "ref: refs/heads/",
    "ghp_",
    "0123456789abcdefghijklmnopqrstuvwxyzAB\n"
);

fn namespace_with_config_ref(
    label: &str,
    config_name: &str,
    config_ref: &str,
) -> (String, crate::io::Repo) {
    let mut files = repository("modules/x", BARE);
    for (path, content) in [
        ("HEAD", SHA),
        (config_name, config_ref),
        ("hooks/main", SHA),
    ] {
        files.push((format!("modules/x/refs/notes/{path}"), content.to_owned()));
    }
    decided(label, &files)
}

#[test]
fn a_ref_named_config_in_capitals_does_not_make_its_namespace_a_repository() {
    let (written, repo) =
        namespace_with_config_ref("config-in-capitals", "Config", "ref: refs/heads/main\n");
    assert_eq!(written, "save=true\nsources=true\n");
    let namespace = repo
        .root()
        .join("home")
        .join(DATABASE)
        .join("modules/x/refs/notes");
    assert!(namespace.join("hooks/main").is_file());
}

#[test]
fn a_symbolic_ref_named_config_does_not_make_its_namespace_a_repository() {
    let (written, repo) =
        namespace_with_config_ref("symbolic-config-ref", "config", "ref: refs/heads/main\n");
    assert_eq!(written, "save=true\nsources=true\n");
    let hooks = repo
        .root()
        .join("home")
        .join(DATABASE)
        .join("modules/x/refs/notes/hooks/main");
    assert!(hooks.is_file());
}

#[test]
fn a_symbolic_ref_naming_a_credential_stops_the_sources_save() {
    let (written, _repo) =
        namespace_with_config_ref("symbolic-credential-target", "config", SYMBOLIC_TARGET);
    assert_eq!(written, "save=true\nsources=false\n");
}

#[test]
fn a_config_that_starts_like_a_symbolic_ref_is_still_scanned_in_a_planted_repository() {
    let config = format!("ref: refs/heads/main\n{TOKEN_REMOTE}");
    let (written, _repo) = decided(
        "symbolic-first-line-config",
        &repository("refs/notes/planted", &config),
    );
    assert_eq!(written, "save=true\nsources=false\n");
}

#[test]
fn a_directory_named_refs_in_another_case_is_scanned_like_any_other() {
    let mut files = repository("modules/x", BARE);
    files.push(("modules/x/Refs/config".to_owned(), TOKEN_REMOTE.to_owned()));
    let (written, _repo) = decided("refs-in-another-case", &files);
    assert_eq!(written, "save=true\nsources=false\n");
}
