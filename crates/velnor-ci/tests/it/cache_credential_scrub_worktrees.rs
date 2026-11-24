// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use crate::cache_private_repo::{GIT_LOCK, decide_with, private_checkout};
use crate::io::Repo;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const DATABASE: &str = ".cargo/git/db/shared-1";
const SAVED: &str = "save=true\nsources=true\n";
const CREDENTIAL_URL: &str = concat!(
    "https://x-access-token:",
    "ghs_",
    "secret@github.com/acme/shared-lib"
);

struct Decided {
    written: String,
    home: PathBuf,
    _repo: Repo,
}

fn decided(label: &str, files: &[(String, String)]) -> Decided {
    let repo = private_checkout(&format!("decide-worktrees-{label}"), GIT_LOCK);
    for (path, content) in files {
        repo.write_unstaged(&format!("home/{path}"), content);
    }
    let (output, written) = decide_with(&repo, Some("true"));
    assert!(output.status.success(), "{label}: {output:?}");
    Decided {
        written,
        home: repo.root().join("home"),
        _repo: repo,
    }
}

fn files(root: &str, entries: &[(&str, &str)]) -> Vec<(String, String)> {
    entries
        .iter()
        .map(|(file, content)| (format!("{root}/{file}"), (*content).to_owned()))
        .collect()
}

#[test]
fn a_linked_worktrees_reflog_is_never_saved_wherever_its_administrative_directory_sits() {
    let logged = format!(
        "{SHA} {SHA} Builder <b@example.invalid> 1 +0000\tcheckout: from {CREDENTIAL_URL}\n"
    );
    for root in [
        format!("{DATABASE}/worktrees/w1"),
        format!("{DATABASE}/modules/vendor/worktrees/w1"),
        format!("{DATABASE}/refs/heads/planted"),
    ] {
        let decided = decided(
            &format!("reflog-{}", root.replace('/', "-")),
            &files(
                &root,
                &[
                    ("HEAD", "ref: refs/heads/main\n"),
                    ("commondir", "../..\n"),
                    ("gitdir", "/work/checkout/.git\n"),
                    ("index", "DIRC"),
                    ("logs/HEAD", &logged),
                ],
            ),
        );
        assert_eq!(decided.written, SAVED, "{root}");
        let administrative = decided.home.join(&root);
        assert!(!administrative.join("logs").exists(), "{root}");
        for kept in ["HEAD", "commondir", "gitdir", "index"] {
            assert!(administrative.join(kept).is_file(), "{root}: {kept}");
        }
    }
}

#[test]
fn a_ref_namespace_holding_a_commondir_ref_is_not_a_repository_and_is_saved() {
    let root = format!("{DATABASE}/modules/x");
    let hooks = "refs/notes/hooks/main";
    let decided = decided(
        "namespace-commondir",
        &files(
            &root,
            &[
                ("HEAD", "ref: refs/heads/main\n"),
                ("config", "[core]\n\tbare = true\n"),
                ("refs/notes/HEAD", SHA),
                ("refs/notes/commondir", SHA),
                (hooks, SHA),
            ],
        ),
    );
    assert_eq!(decided.written, SAVED);
    assert!(decided.home.join(&root).join(hooks).is_file());
}
