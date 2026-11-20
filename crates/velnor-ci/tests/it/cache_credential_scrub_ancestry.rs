// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache_credential_scrub::{sources_save_goes_on, sources_save_stops};
use crate::cache_private_repo::{GIT_LOCK, decide_with, private_checkout};

#[test]
fn a_hex_token_in_a_hook_stops_the_sources_save() {
    sources_save_stops(&[
        (
            "hook-40-hex",
            ".cargo/git/db/shared-1/hooks/post-fetch",
            "#!/bin/sh\nTOKEN=0123456789abcdef0123456789abcdef01234567\n",
        ),
        (
            "nested-hook-40-hex",
            ".cargo/git/db/shared-1/hooks/lib/auth",
            "TOKEN=0123456789abcdef0123456789abcdef01234567\n",
        ),
    ]);
}

#[test]
fn a_token_in_a_helper_nested_under_hooks_stops_the_sources_save() {
    sources_save_stops(&[
        (
            "nested-hook-token",
            ".cargo/git/db/shared-1/hooks/lib/auth",
            "export TOKEN=ghs_0123456789abcdefghijklmnopqrstuvwxyzAB\n",
        ),
        (
            "deeply-nested-hook-token",
            ".cargo/git/db/shared-1/hooks/lib/deep/auth",
            "export TOKEN=ghs_0123456789abcdefghijklmnopqrstuvwxyzAB\n",
        ),
        (
            "nested-hook-userinfo-url",
            ".cargo/git/db/shared-1/hooks/lib/auth",
            "git push https://x-access-token:secret@github.com/acme/shared-lib\n",
        ),
    ]);
}

#[test]
fn a_plain_helper_nested_under_hooks_is_saved() {
    sources_save_goes_on(&[(
        "plain-nested-hook",
        ".cargo/git/db/shared-1/hooks/lib/helper",
        "#!/bin/sh\necho done 0123456789abcdef\n",
    )]);
}

#[test]
fn a_percent_encoded_token_in_a_remote_path_stops_the_sources_save() {
    sources_save_stops(&[
        (
            "encoded-prefix-token",
            ".cargo/git/db/shared-1/config",
            "[remote \"origin\"]\n\turl = https://github.example/%67hp_0123456789abcdefghijklmnopqrstuvwxyzAB/shared-lib\n",
        ),
        (
            "double-encoded-prefix-token",
            ".cargo/git/db/shared-1/config",
            "[remote \"origin\"]\n\turl = https://github.example/%2567hp_0123456789abcdefghijklmnopqrstuvwxyzAB/shared-lib\n",
        ),
        (
            "encoded-hex-token",
            ".cargo/git/db/shared-1/config",
            "[remote \"origin\"]\n\turl = https://github.example/%30123456789abcdef0123456789abcdef01234567/shared-lib\n",
        ),
        (
            "encoded-token-in-a-hook-url",
            ".cargo/git/db/shared-1/hooks/post-fetch",
            "# https://github.example/%67hp_0123456789abcdefghijklmnopqrstuvwxyzAB/shared-lib\n",
        ),
    ]);
}

#[test]
fn a_percent_escape_in_a_plain_remote_path_is_saved() {
    sources_save_goes_on(&[(
        "encoded-space-in-path",
        ".cargo/git/db/shared-1/config",
        "[remote \"origin\"]\n\turl = https://github.example/acme/my%20lib\n",
    )]);
}

#[cfg(unix)]
fn stops_for_symlinked_ancestor(name: &str, ancestor: &str, outside: &str, planted: &str) {
    let repo = private_checkout(name, GIT_LOCK);
    repo.write_unstaged(&format!("home/outside/{planted}"), "[core]\n\tbare = true\n");
    let link = repo.root().join("home").join(ancestor);
    std::fs::create_dir_all(link.parent().unwrap_or_else(|| unreachable!()))
        .unwrap_or_else(|e| unreachable!("{e}"));
    std::os::unix::fs::symlink(repo.root().join("home").join(outside), &link)
        .unwrap_or_else(|e| unreachable!("{e}"));
    let (output, written) = decide_with(&repo, Some("true"));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=false\n", "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("symbolic link"),
        "{output:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_git_directory_above_the_saved_root_stops_the_sources_save() {
    stops_for_symlinked_ancestor(
        "decide-credential-symlinked-git-ancestor",
        ".cargo/git",
        "outside",
        "db/shared-1/config",
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_registry_directory_above_the_saved_root_stops_the_sources_save() {
    stops_for_symlinked_ancestor(
        "decide-credential-symlinked-registry-ancestor",
        ".cargo/registry",
        "outside",
        "cache/index.crates.io-1/crate.crate",
    );
}

#[cfg(unix)]
#[test]
fn a_symlinked_cargo_home_stops_the_sources_save() {
    stops_for_symlinked_ancestor(
        "decide-credential-symlinked-cargo-home",
        ".cargo",
        "outside",
        "git/db/shared-1/config",
    );
}
