// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use crate::cache_private_repo::{GIT_LOCK, decide_with, private_checkout};
use crate::io::Repo;

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const DATABASE: &str = ".cargo/git/db/shared-1";
const MARKERS: [(&str, &str); 2] = [("HEAD", "ref: refs/heads/main\n"), ("config", BARE)];
const BARE: &str = "[core]\n\tbare = true\n";
const TOKEN_REMOTE: &str = concat!(
    "[remote \"origin\"]\n\turl = https://x-access-token:",
    "ghs_",
    "secret@github.com/acme/shared-lib\n"
);

struct Decided {
    written: String,
    home: PathBuf,
    _repo: Repo,
}

fn decided(label: &str, files: &[(String, String)]) -> Decided {
    let repo = private_checkout(&format!("decide-trees-{label}"), GIT_LOCK);
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

fn repository(root: &str, extra: &[(&str, &str)]) -> Vec<(String, String)> {
    MARKERS
        .iter()
        .chain(extra)
        .map(|(file, content)| (format!("{root}/{file}"), (*content).to_owned()))
        .collect()
}

const SAVED: &str = "save=true\nsources=true\n";
const CREDENTIAL_URL: &str = concat!(
    "https://x-access-token:",
    "ghs_",
    "secret@github.com/acme/shared-lib"
);
const REFUSED: &str = "save=true\nsources=false\n";

#[test]
fn a_ref_namespace_holding_a_head_ref_is_not_a_module_and_is_saved() {
    for namespace in [
        "notes",
        "stash",
        "replace",
        "bisect",
        "pull",
        "custom/nested",
    ] {
        let head = format!("refs/{namespace}/HEAD");
        let hooks = format!("refs/{namespace}/hooks/main");
        let root = format!("{DATABASE}/modules/x");
        let decided = decided(
            &format!("namespace-{}", namespace.replace('/', "-")),
            &repository(&root, &[(&head, SHA), (&hooks, SHA)]),
        );
        assert_eq!(decided.written, SAVED, "{namespace}");
        assert!(
            decided.home.join(&root).join(&hooks).is_file(),
            "{namespace}"
        );
    }
}

#[test]
fn a_symbolic_ref_named_config_does_not_make_its_namespace_a_repository() {
    let root = format!("{DATABASE}/modules/x");
    let decided = decided(
        "symbolic-config-ref",
        &repository(
            &root,
            &[
                ("refs/notes/HEAD", SHA),
                ("refs/notes/config", "ref: refs/heads/main\n"),
                ("refs/notes/hooks/main", SHA),
            ],
        ),
    );
    assert_eq!(decided.written, SAVED);
    assert!(
        decided
            .home
            .join(&root)
            .join("refs/notes/hooks/main")
            .is_file()
    );
}

#[test]
fn a_config_that_starts_like_a_symbolic_ref_is_still_scanned_in_a_planted_repository() {
    let root = format!("{DATABASE}/refs/notes/planted");
    let config = format!("ref: refs/heads/main\n{TOKEN_REMOTE}");
    let stopped = decided(
        "symbolic-first-line-config",
        &repository(&root, &[("config", &config)]),
    );
    assert_eq!(stopped.written, REFUSED);
}

#[test]
fn a_git_directory_planted_in_a_refs_tree_is_still_scanned() {
    for place in [
        "refs/heads/planted",
        "refs/notes/planted",
        "refs/custom/a/planted",
    ] {
        let root = format!("{DATABASE}/{place}");
        let stopped = decided(
            &format!("planted-config-{}", place.replace('/', "-")),
            &repository(&root, &[("config", TOKEN_REMOTE)]),
        );
        assert_eq!(stopped.written, REFUSED, "{place}");
        let dropped = decided(
            &format!("planted-hooks-{}", place.replace('/', "-")),
            &repository(&root, &[("hooks/post-fetch", &format!("TOKEN={SHA}\n"))]),
        );
        assert_eq!(dropped.written, SAVED, "{place}");
        assert!(!dropped.home.join(&root).join("hooks").exists(), "{place}");
        assert!(dropped.home.join(&root).join("HEAD").is_file(), "{place}");
    }
}

#[test]
fn a_ref_namespace_named_objects_does_not_make_its_parent_a_repository() {
    for namespace in ["notes", "heads/x", "custom/nested"] {
        for objects in ["objects/main", "objects/pack/main", "objects/info/main"] {
            let head = format!("refs/{namespace}/HEAD");
            let named = format!("refs/{namespace}/{objects}");
            let hooks = format!("refs/{namespace}/hooks/main");
            let root = format!("{DATABASE}/modules/x");
            let decided = decided(
                &format!(
                    "objects-{}-{}",
                    namespace.replace('/', "-"),
                    objects.replace('/', "-")
                ),
                &repository(&root, &[(&head, SHA), (&named, SHA), (&hooks, SHA)]),
            );
            assert_eq!(decided.written, SAVED, "{namespace} {objects}");
            assert!(
                decided.home.join(&root).join(&hooks).is_file(),
                "{namespace}"
            );
            assert!(
                decided.home.join(&root).join(&named).is_file(),
                "{namespace}"
            );
        }
    }
}

#[test]
fn a_git_directory_with_an_object_store_planted_in_a_refs_tree_is_still_found() {
    for place in ["refs/heads/planted", "refs/notes/planted"] {
        let root = format!("{DATABASE}/{place}");
        let dropped = decided(
            &format!("planted-store-{}", place.replace('/', "-")),
            &[
                (format!("{root}/HEAD"), "ref: refs/heads/main\n".to_owned()),
                (format!("{root}/objects/info/packs"), String::new()),
                (
                    format!("{root}/objects/pack/pack-1.pack"),
                    "pack".to_owned(),
                ),
                (format!("{root}/hooks/post-fetch"), format!("TOKEN={SHA}\n")),
            ],
        );
        assert_eq!(dropped.written, SAVED, "{place}");
        assert!(!dropped.home.join(&root).join("hooks").exists(), "{place}");
        assert!(dropped.home.join(&root).join("HEAD").is_file(), "{place}");
    }
}

#[test]
fn hooks_are_never_saved_wherever_the_git_directory_sits() {
    let hook = format!("TOKEN={SHA}\n");
    for root in [
        DATABASE.to_owned(),
        format!("{DATABASE}/vendor/.git"),
        format!("{DATABASE}/vendor/deeper/.git"),
        format!("{DATABASE}/modules/vendor"),
        format!("{DATABASE}/modules/vendor/modules/deep"),
        format!("{DATABASE}/worktrees/w1/modules/vendor"),
        ".cargo/registry/index/index.crates.io-1/.git".to_owned(),
        ".cargo/registry/index/index.crates.io-1/nested/.git".to_owned(),
    ] {
        let decided = decided(
            &format!("hooks-{}", root.replace('/', "-")),
            &repository(
                &root,
                &[("hooks/post-fetch", &hook), ("hooks/sub/x", &hook)],
            ),
        );
        assert_eq!(decided.written, SAVED, "{root}");
        assert!(!decided.home.join(&root).join("hooks").exists(), "{root}");
        assert!(decided.home.join(&root).join("config").is_file(), "{root}");
    }
}

#[test]
fn hooks_with_every_reported_evasion_are_absent_from_the_saved_sources() {
    for (label, hook) in [
        (
            "command-substitution",
            concat!("echo ghp_", "0123456789$(true)abcdefghijklmnopqrstuvwxyz\n"),
        ),
        (
            "ansi-c-escape",
            "echo ghp$'\\x5f'0123456789abcdefghijklmnopqrstuvwxyzAB\n",
        ),
        (
            "empty-expansion",
            concat!(
                "echo ghp_",
                "0123456789${EMPTY}abcdefghijklmnopqrstuvwxyz\n"
            ),
        ),
        (
            "nested-expansion",
            concat!(
                "echo ghp_",
                "0123456789${A:-${B}}abcdefghijklmnopqrstuvwxyz\n"
            ),
        ),
        (
            "quote-split",
            concat!("echo ghp_", "0123456789\"abcdefghijklmnopqrstuvwxyzAB\"\n"),
        ),
        (
            "plain",
            concat!("echo ghp_", "0123456789abcdefghijklmnopqrstuvwxyzAB\n"),
        ),
    ] {
        let decided = decided(
            &format!("evasion-{label}"),
            &repository(DATABASE, &[("hooks/post-fetch", hook)]),
        );
        assert_eq!(decided.written, SAVED, "{label}");
        assert!(
            !decided.home.join(DATABASE).join("hooks").exists(),
            "{label}"
        );
    }
}

#[test]
fn fetch_head_and_reflogs_are_never_saved_wherever_the_git_directory_sits() {
    let roots = [
        DATABASE.to_owned(),
        format!("{DATABASE}/vendor/.git"),
        format!("{DATABASE}/modules/vendor"),
        format!("{DATABASE}/worktrees/w1/modules/vendor"),
        ".cargo/registry/index/index.crates.io-1/.git".to_owned(),
    ];
    for root in roots {
        let fetched = format!("{SHA}\t\tbranch 'main' of {CREDENTIAL_URL}\n");
        let logged = format!(
            "{SHA} {SHA} Builder <b@example.invalid> 1 +0000\tclone: from {CREDENTIAL_URL}\n"
        );
        let decided = decided(
            &format!("left-out-{}", root.replace('/', "-")),
            &repository(
                &root,
                &[
                    ("FETCH_HEAD", &fetched),
                    ("logs/HEAD", &logged),
                    ("logs/refs/remotes/origin/main", &logged),
                    ("packed-refs", &format!("{SHA} refs/heads/main\n")),
                    ("refs/heads/main", SHA),
                ],
            ),
        );
        assert_eq!(decided.written, SAVED, "{root}");
        let git = decided.home.join(&root);
        assert!(!git.join("FETCH_HEAD").exists(), "{root}");
        assert!(!git.join("logs").exists(), "{root}");
        for kept in ["HEAD", "config", "packed-refs", "refs/heads/main"] {
            assert!(git.join(kept).is_file(), "{root}: {kept}");
        }
    }
}

#[test]
fn a_credential_in_a_git_config_still_stops_the_save_beside_a_left_out_fetch_head() {
    let stopped = decided(
        "config-beside-fetch-head",
        &repository(
            DATABASE,
            &[
                ("config", TOKEN_REMOTE),
                ("FETCH_HEAD", SHA),
                ("logs/HEAD", SHA),
            ],
        ),
    );
    assert_eq!(stopped.written, REFUSED);
}

#[test]
fn a_ref_named_hooks_survives_the_hooks_exclusion() {
    for root in [
        DATABASE.to_owned(),
        format!("{DATABASE}/modules/vendor"),
        format!("{DATABASE}/modules/vendor/modules/deep"),
        format!("{DATABASE}/modules/refs"),
        ".cargo/registry/index/index.crates.io-1/.git".to_owned(),
    ] {
        let refs = [
            "refs/heads/hooks/main",
            "refs/remotes/origin/hooks/main",
            "refs/tags/hooks/v1",
            "refs/heads/logs/main",
            "refs/remotes/origin/FETCH_HEAD",
        ];
        let files: Vec<(&str, &str)> = refs.iter().map(|path| (*path, SHA)).collect();
        let kept = decided(
            &format!("ref-named-hooks-{}", root.replace('/', "-")),
            &repository(&root, &files),
        );
        assert_eq!(kept.written, SAVED, "{root}");
        for path in refs {
            assert!(kept.home.join(&root).join(path).is_file(), "{root}: {path}");
        }
    }
}

#[test]
fn a_double_form_encoded_basic_header_in_a_config_comment_stops_the_sources_save() {
    for (label, comment) in [
        (
            "two-layers",
            "# Authorization%253A%2BBasic%2BdXNlcjpwYXNz\n",
        ),
        ("one-layer", "# Authorization%3A+Basic+dXNlcjpwYXNz\n"),
    ] {
        let stopped = decided(
            &format!("form-{label}"),
            &repository(DATABASE, &[("config", &format!("{BARE}{comment}"))]),
        );
        assert_eq!(stopped.written, REFUSED, "{label}");
    }
}

fn config_with_comment(comment: &str) -> Vec<(String, String)> {
    let mut files = repository(DATABASE, &[]);
    files.retain(|(path, _)| !path.ends_with("/config"));
    files.push((format!("{DATABASE}/config"), format!("{BARE}# {comment}\n")));
    files
}

fn adversarial_comments(cases: &[(&str, String, &str)]) {
    for (label, comment, outcome) in cases {
        let decided = decided(
            &format!("adversarial-{label}"),
            &config_with_comment(comment),
        );
        assert_eq!(decided.written, *outcome, "{label}");
    }
}

fn mebibyte_of(unit: &str) -> String {
    unit.repeat(((1 << 20) - 1024) / unit.len())
}

#[test]
fn adversarial_one_mebibyte_credential_runs_are_decided_without_quadratic_work() {
    adversarial_comments(&[
        ("run-of-basic", mebibyte_of("authorizationbasic"), REFUSED),
        ("schemes", mebibyte_of("http://"), REFUSED),
        ("tokens", mebibyte_of("ghp_"), REFUSED),
    ]);
}

#[test]
fn adversarial_one_mebibyte_headers_are_decided_without_quadratic_work() {
    let near_cap = format!("authorization basic {} ", "A".repeat(480));
    adversarial_comments(&[
        (
            "short-headers",
            mebibyte_of("Authorization: Basic abcd "),
            SAVED,
        ),
        ("headers-near-the-cap", mebibyte_of(&near_cap), SAVED),
    ]);
}

#[test]
fn adversarial_one_mebibyte_plain_runs_are_decided_without_quadratic_work() {
    adversarial_comments(&[
        ("hosts", mebibyte_of("http://a/"), SAVED),
        ("escapes", mebibyte_of("%25"), SAVED),
    ]);
}
