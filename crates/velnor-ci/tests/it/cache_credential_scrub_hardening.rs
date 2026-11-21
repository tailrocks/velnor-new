// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache_credential_scrub::{sources_save_goes_on, sources_save_stops};
use crate::cache_private_repo::{GIT_LOCK, decide_with, private_checkout};

#[test]
fn an_ssh_username_that_is_a_token_stops_the_sources_save() {
    sources_save_stops(&[
        (
            "ssh-ghp-username",
            ".cargo/git/db/shared-1/config",
            "[remote \"origin\"]\n\turl = ssh://ghp_0123456789abcdefghijklmnopqrstuvwxyzAB@github.com/acme/shared-lib\n",
        ),
        (
            "ssh-gho-username",
            ".cargo/git/db/shared-1/config",
            "[remote \"origin\"]\n\turl = ssh://gho_0123456789abcdefghijklmnopqrstuvwxyzAB@github.com/acme/shared-lib\n",
        ),
        (
            "ssh-ghs-username",
            ".cargo/git/db/shared-1/config",
            "[remote \"origin\"]\n\turl = ssh://ghs_0123456789abcdefghijklmnopqrstuvwxyzAB@github.com/acme/shared-lib\n",
        ),
        (
            "ssh-ghu-username",
            ".cargo/git/db/shared-1/config",
            "[remote \"origin\"]\n\turl = ssh://ghu_0123456789abcdefghijklmnopqrstuvwxyzAB@github.com/acme/shared-lib\n",
        ),
        (
            "ssh-ghr-username",
            ".cargo/git/db/shared-1/config",
            "[remote \"origin\"]\n\turl = ssh://ghr_0123456789abcdefghijklmnopqrstuvwxyzAB@github.com/acme/shared-lib\n",
        ),
        (
            "ssh-github-pat-username",
            ".cargo/git/db/shared-1/config",
            "[remote \"origin\"]\n\turl = ssh://github_pat_11AAAAAAA00123456789abcdefghijklmnopqrstuvwxyzAB@github.com/acme/shared-lib\n",
        ),
        (
            "ssh-40-hex-username",
            ".cargo/git/db/shared-1/config",
            "[remote \"origin\"]\n\turl = ssh://0123456789abcdef0123456789abcdef01234567@github.com/acme/shared-lib\n",
        ),
    ]);
}

#[test]
fn known_git_hosting_usernames_on_ssh_remotes_are_saved() {
    sources_save_goes_on(&[
        (
            "ssh-gitlab-username",
            ".cargo/git/db/shared-1/config",
            "[remote \"origin\"]\n\turl = ssh://gitlab@gitlab.com/acme/shared-lib\n",
        ),
        (
            "ssh-gitea-username",
            ".cargo/git/db/shared-1/config",
            "[remote \"origin\"]\n\turl = ssh://gitea@gitea.example/acme/shared-lib\n",
        ),
    ]);
}

#[test]
fn a_token_in_a_remote_url_query_or_path_stops_the_sources_save() {
    sources_save_stops(&[
        (
            "https-query-token",
            ".cargo/git/db/shared-1/config",
            "[remote \"origin\"]\n\turl = https://github.example/acme/shared?access_token=secret\n",
        ),
        (
            "ssh-query-token",
            ".cargo/git/db/shared-1/config",
            "[remote \"origin\"]\n\turl = ssh://git@github.example/acme/shared?token=secret\n",
        ),
        (
            "https-path-token",
            ".cargo/git/db/shared-1/config",
            "[remote \"origin\"]\n\turl = https://github.example/ghp_0123456789abcdefghijklmnopqrstuvwxyzAB/shared-lib\n",
        ),
        (
            "https-path-40-hex",
            ".cargo/git/db/shared-1/config",
            "[remote \"origin\"]\n\turl = https://github.example/0123456789abcdef0123456789abcdef01234567/shared-lib\n",
        ),
    ]);
}

#[test]
fn a_planted_token_in_a_git_file_the_scan_used_to_skip_stops_the_sources_save() {
    sources_save_stops(&[
        (
            "gitconfig-extraheader",
            ".cargo/git/db/shared-1/.gitconfig",
            "[http]\n\textraheader = AUTHORIZATION: basic eC1hY2Nlc3M6c2VjcmV0\n",
        ),
        (
            "config-bak-proxy",
            ".cargo/git/db/shared-1/config.bak",
            "[http]\n\tproxy = http://attacker.invalid:3128\n",
        ),
    ]);
}

#[test]
fn a_credential_url_embedded_in_a_word_stops_the_sources_save() {
    sources_save_stops(&[
        (
            "embedded-userinfo-url",
            ".cargo/git/db/shared-1/config",
            "[core]\n\tbare = true\n# (https://x-access-token:secret@github.com/acme/shared-lib)\n",
        ),
        (
            "embedded-query-token-url",
            ".cargo/git/db/shared-1/config",
            "[core]\n\tbare = true\n# (https://github.example/acme/shared?access_token=secret)\n",
        ),
        (
            "embedded-url-after-a-plain-one",
            ".cargo/git/db/shared-1/config",
            "[core]\n\tbare = true\n# (https://example.org/docs)(https://user@github.example/acme/shared-lib)\n",
        ),
        (
            "embedded-foreign-scheme-url",
            ".cargo/git/db/shared-1/config",
            "[core]\n\tbare = true\n# (ftp://user:secret@example.org/file)\n",
        ),
    ]);
}

#[test]
fn a_token_in_a_config_comment_or_header_or_a_gitlab_token_stops_the_sources_save() {
    sources_save_stops(&[
        (
            "config-comment-token",
            ".cargo/git/db/shared-1/config",
            "[core]\n\tbare = true\n# ghp_0123456789abcdefghijklmnopqrstuvwxyzAB\n",
        ),
        (
            "config-header-only-section-token",
            ".cargo/git/db/shared-1/config",
            "[credential \"https://ghp_0123456789abcdefghijklmnopqrstuvwxyzAB@github.com\"]\n",
        ),
        (
            "config-remote-name-token",
            ".cargo/git/db/shared-1/config",
            "[remote \"ghp_0123456789abcdefghijklmnopqrstuvwxyzAB\"]\n\turl = https://github.com/acme/shared-lib\n",
        ),
        (
            "gitlab-token-in-a-config-comment",
            ".cargo/git/db/shared-1/config",
            "[core]\n\tbare = true\n# glpat-abcdefghijklmnopqrst\n",
        ),
    ]);
}

#[cfg(unix)]
fn sources_save_stops_for_symlink(name: &str, link: &str, target: &str) {
    let repo = private_checkout(name, GIT_LOCK);
    repo.write_unstaged(
        "home/.cargo/git/db/shared-1/innocuous",
        "[http]\n\tproxy = http://attacker.invalid:3128\n",
    );
    let link = repo.root().join("home").join(link);
    std::fs::create_dir_all(link.parent().unwrap_or_else(|| unreachable!()))
        .unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink(target, &link).unwrap_or_else(|e| unreachable!("{e}"));
    let (output, written) = decide_with(&repo, Some("true"));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=false\n", "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("credential"),
        "{output:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_git_config_stops_the_sources_save() {
    sources_save_stops_for_symlink(
        "decide-credential-symlinked-config",
        ".cargo/git/db/shared-1/config",
        "innocuous",
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_git_file_stops_the_sources_save() {
    sources_save_stops_for_symlink(
        "decide-credential-symlinked-fetch-head",
        ".cargo/git/db/shared-1/FETCH_HEAD",
        "innocuous",
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_escaping_the_saved_tree_stops_the_sources_save() {
    sources_save_stops_for_symlink(
        "decide-credential-escaping-symlink",
        ".cargo/git/db/shared-1/escape",
        "/",
    );
}
