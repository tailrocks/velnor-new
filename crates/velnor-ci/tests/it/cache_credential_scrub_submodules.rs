// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use crate::cache_private_repo::{GIT_LOCK, decide_with, private_checkout};
use crate::io::Repo;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const DATABASE: &str = ".cargo/git/db/shared-1";
const BARE: &str = "[core]\n\tbare = true\n";
const SAVED: &str = "save=true\nsources=true\n";
const REFUSED: &str = "save=true\nsources=false\n";
const HOOK: &str = "TOKEN=0123456789abcdef0123456789abcdef01234567\n";

struct Decided {
    written: String,
    home: PathBuf,
    _repo: Repo,
}

fn decided(label: &str, files: &[(String, &str)]) -> Decided {
    let repo = private_checkout(&format!("decide-submodules-{label}"), GIT_LOCK);
    for (path, content) in files {
        repo.write_unstaged(&format!("home/{DATABASE}/{path}"), content);
    }
    let (output, written) = decide_with(&repo, Some("true"));
    assert!(output.status.success(), "{label}: {output:?}");
    Decided {
        written,
        home: repo.root().join("home").join(DATABASE),
        _repo: repo,
    }
}

fn at(root: &str, file: &str) -> String {
    if root.is_empty() {
        file.to_owned()
    } else {
        format!("{root}/{file}")
    }
}

fn repository(root: &str) -> Vec<(String, &'static str)> {
    vec![
        (at(root, "HEAD"), "ref: refs/heads/main\n"),
        (at(root, "config"), BARE),
        (at(root, "objects/pack/pack-1.pack"), "pack"),
        (at(root, "hooks/post-fetch"), HOOK),
        (at(root, "logs/HEAD"), SHA),
    ]
}

#[test]
fn a_submodule_repository_named_hooks_or_logs_is_saved_and_only_its_own_hooks_and_logs_go() {
    for name in ["hooks", "logs"] {
        for root in [
            format!("modules/{name}"),
            format!("modules/vendor/modules/{name}"),
            format!("modules/{name}/modules/{name}"),
        ] {
            let decided = decided(
                &format!("{name}-{}", root.replace('/', "-")),
                &repository(&root),
            );
            assert_eq!(decided.written, SAVED, "{root}");
            let kept = decided.home.join(&root);
            for file in ["HEAD", "config", "objects/pack/pack-1.pack"] {
                assert!(kept.join(file).is_file(), "{root}: {file}");
            }
            assert!(!kept.join("hooks").exists(), "{root}");
            assert!(!kept.join("logs").exists(), "{root}");
        }
    }
}

#[test]
fn a_hooks_directory_holding_a_git_directory_is_cleaned_not_deleted() {
    let mut files = repository("");
    files.extend(repository("hooks"));
    let decided = decided("hooks-is-a-repository", &files);
    assert_eq!(decided.written, SAVED);
    assert!(decided.home.join("hooks/HEAD").is_file());
    assert!(
        decided
            .home
            .join("hooks/objects/pack/pack-1.pack")
            .is_file()
    );
    assert!(!decided.home.join("hooks/hooks").exists());
    assert!(!decided.home.join("hooks/logs").exists());
    assert!(!decided.home.join("logs").exists());
}

#[test]
fn ref_files_named_config_or_credentials_do_not_stop_the_sources_save() {
    let refs = [
        "refs/heads/topic/config",
        "refs/remotes/origin/config",
        "refs/tags/credentials",
        "refs/heads/topic/credentials",
        "refs/heads/topic/HEAD",
    ];
    for root in ["", "modules/vendor"] {
        let mut files = repository(root);
        files.extend(refs.iter().map(|path| (at(root, path), SHA)));
        let decided = decided(&format!("ref-files-{}", root.replace('/', "-")), &files);
        assert_eq!(decided.written, SAVED, "{root}");
        for path in refs {
            assert!(
                decided.home.join(at(root, path)).is_file(),
                "{root}: {path}"
            );
        }
    }
}

#[test]
fn a_hooks_tree_nested_beyond_the_depth_limit_refuses_the_save() {
    let deep = ["d"; 16].join("/");
    let mut files = repository("");
    files.push((format!("hooks/{deep}/x"), HOOK));
    let decided = decided("hooks-too-deep", &files);
    assert_eq!(decided.written, REFUSED);
}

#[test]
fn a_nested_hooks_tree_within_the_depth_limit_is_removed() {
    let nested = ["d"; 8].join("/");
    let mut files = repository("");
    files.push((format!("hooks/{nested}/x"), HOOK));
    let decided = decided("hooks-nested", &files);
    assert_eq!(decided.written, SAVED);
    assert!(!decided.home.join("hooks").exists());
}
