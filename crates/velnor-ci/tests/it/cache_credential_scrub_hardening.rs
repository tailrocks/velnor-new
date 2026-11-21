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
            "fetch-head-userinfo",
            ".cargo/git/db/shared-1/FETCH_HEAD",
            "0123456789abcdef0123456789abcdef01234567\t\tbranch 'main' of https://x-access-token:ghs_secret@github.com/acme/shared-lib\n",
        ),
        (
            "fetch-head-token",
            ".cargo/git/db/shared-1/FETCH_HEAD",
            "0123456789abcdef0123456789abcdef01234567\t\tbranch 'main' ghp_0123456789abcdefghijklmnopqrstuvwxyzAB\n",
        ),
        (
            "packed-refs-token",
            ".cargo/git/db/shared-1/packed-refs",
            "# pack-refs with: peeled fully-peeled sorted\n0123456789abcdef0123456789abcdef01234567 refs/tags/ghs_0123456789abcdefghijklmnopqrstuvwxyzAB\n",
        ),
        (
            "hook-token",
            ".cargo/git/db/shared-1/hooks/post-fetch",
            "#!/bin/sh\necho ghp_0123456789abcdefghijklmnopqrstuvwxyzAB\n",
        ),
        (
            "hook-userinfo-url",
            ".cargo/git/db/shared-1/hooks/post-fetch",
            "#!/bin/sh\ngit push https://x-access-token:ghs_secret@github.com/acme/shared-lib\n",
        ),
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
fn plain_git_files_the_scan_reads_are_saved() {
    sources_save_goes_on(&[
        (
            "plain-fetch-head",
            ".cargo/git/db/shared-1/FETCH_HEAD",
            "0123456789abcdef0123456789abcdef01234567\t\tbranch 'main' of https://github.com/acme/shared-lib\n",
        ),
        (
            "plain-packed-refs",
            ".cargo/git/db/shared-1/packed-refs",
            "# pack-refs with: peeled fully-peeled sorted\n0123456789abcdef0123456789abcdef01234567 refs/heads/main\n",
        ),
        (
            "plain-hook",
            ".cargo/git/db/shared-1/hooks/post-fetch.sample",
            "#!/bin/sh\necho done\n",
        ),
    ]);
}

const FSMONITOR_WATCHMAN_HEAD: &str = "#!/usr/bin/perl

use strict;
use warnings;
use IPC::Open2;

# An example hook script to integrate Watchman
# (https://facebook.github.io/watchman/) with git to speed up detecting
# new and modified files.
#
# The hook is passed a version (currently 2) and last update token
# formatted as a string and outputs to stdout a new update token and
# all files that have been modified since the update token. Paths must
# be relative to the root of the working tree and separated by a single NUL.
#
# To enable this hook, rename this file to \"query-watchman\" and set
# 'git config core.fsmonitor .git/hooks/query-watchman'
#
my ($version, $last_update_token) = @ARGV;
";

#[test]
fn git_stock_hook_samples_with_a_parenthesised_url_are_saved() {
    sources_save_goes_on(&[
        (
            "stock-fsmonitor-watchman-sample",
            ".cargo/git/db/shared-1/hooks/fsmonitor-watchman.sample",
            FSMONITOR_WATCHMAN_HEAD,
        ),
        (
            "url-in-brackets-and-trailing-punctuation",
            ".cargo/git/db/shared-1/hooks/post-fetch",
            "# see [https://example.org/docs], <https://example.org/a>, https://example.org/b.\n",
        ),
    ]);
}

#[test]
fn a_credential_url_embedded_in_a_word_stops_the_sources_save() {
    sources_save_stops(&[
        (
            "embedded-userinfo-url",
            ".cargo/git/db/shared-1/hooks/post-fetch",
            "# (https://x-access-token:secret@github.com/acme/shared-lib)\n",
        ),
        (
            "embedded-query-token-url",
            ".cargo/git/db/shared-1/hooks/post-fetch",
            "# (https://github.example/acme/shared?access_token=secret)\n",
        ),
        (
            "embedded-url-after-a-plain-one",
            ".cargo/git/db/shared-1/hooks/post-fetch",
            "# (https://example.org/docs)(https://user@github.example/acme/shared-lib)\n",
        ),
        (
            "embedded-foreign-scheme-url",
            ".cargo/git/db/shared-1/hooks/post-fetch",
            "# (ftp://user:secret@example.org/file)\n",
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
            "gitlab-token-in-hook",
            ".cargo/git/db/shared-1/hooks/post-fetch",
            "#!/bin/sh\necho glpat-abcdefghijklmnopqrst\n",
        ),
    ]);
}

#[test]
fn a_ref_name_that_only_contains_a_token_prefix_is_saved() {
    sources_save_goes_on(&[(
        "packed-refs-prefix-inside-a-word",
        ".cargo/git/db/shared-1/packed-refs",
        "0123456789abcdef0123456789abcdef01234567 refs/heads/weighs_x\n",
    )]);
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
