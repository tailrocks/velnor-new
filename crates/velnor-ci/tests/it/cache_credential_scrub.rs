// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Output;

use crate::cache_private_repo::{
    GIT_LOCK, PRIVATE, TOKEN, decide_in, decide_with, decide_without_home, private_checkout,
};
use crate::fetch::serve_body;

fn saved_sources_with(name: &str, relative: &str, content: &str) -> (Output, String) {
    let repo = private_checkout(name, GIT_LOCK);
    repo.write_unstaged(&format!("home/{relative}"), content);
    let api = serve_body(200, PRIVATE).unwrap_or_else(|e| unreachable!("{e}"));
    decide_with(&repo, Some(&api), Some(TOKEN))
}

fn sources_save_stops(cases: &[(&str, &str, &str)]) {
    for (label, relative, content) in cases {
        let (output, written) =
            saved_sources_with(&format!("decide-credential-{label}"), relative, content);
        assert!(output.status.success(), "{label}: {output:?}");
        assert_eq!(written, "save=true\nsources=false\n", "{label}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("credential"),
            "{label}: {output:?}"
        );
    }
}

fn sources_save_goes_on(cases: &[(&str, &str, &str)]) {
    for (label, relative, content) in cases {
        let (output, written) =
            saved_sources_with(&format!("decide-outside-{label}"), relative, content);
        assert!(output.status.success(), "{label}: {output:?}");
        assert_eq!(written, "save=true\nsources=true\n", "{label}: {output:?}");
    }
}

#[test]
fn a_credential_file_inside_the_saved_sources_paths_stops_the_sources_save_only() {
    sources_save_stops(&[
        (
            "git-credentials",
            ".cargo/git/db/shared-1/.git-credentials",
            "https://x-access-token:ghs_secret@github.com\n",
        ),
        (
            "netrc",
            ".cargo/git/db/shared-1/.netrc",
            "machine github.com login x password ghs_secret\n",
        ),
        (
            "registry-credentials",
            ".cargo/registry/cache/index.crates.io-1/credentials.toml",
            "[registry]\ntoken = \"secret\"\n",
        ),
    ]);
}

#[test]
fn a_token_in_a_saved_git_config_stops_the_sources_save_only() {
    sources_save_stops(&[
        (
            "token-in-config-url",
            ".cargo/git/db/shared-1/config",
            "[remote \"origin\"]\n\turl = https://x-access-token:ghs_secret@github.com/acme/shared-lib\n",
        ),
        (
            "extraheader",
            ".cargo/git/db/shared-1/config",
            "[http]\n\textraheader = AUTHORIZATION: basic eC1hY2Nlc3M6c2VjcmV0\n",
        ),
    ]);
}

#[test]
fn each_credential_signal_alone_stops_the_sources_save() {
    let config = ".cargo/git/db/shared-1/config";
    sources_save_stops(&[
        (
            "bare-credentials-file",
            ".cargo/git/db/shared-1/credentials",
            "x\n",
        ),
        (
            "userinfo-only",
            config,
            "[remote \"origin\"]\n\turl = https://deploy@github.com/acme/shared-lib\n",
        ),
        (
            "plain-http-userinfo-only",
            config,
            "[remote \"origin\"]\n\turl = http://deploy@github.com/acme/shared-lib\n",
        ),
        ("extraheader-only", config, "[http]\n\textraheader = x\n"),
        (
            "authorization-only",
            config,
            "[http]\n\theader = Authorization: x\n",
        ),
        (
            "x-access-token-only",
            config,
            "[credential]\n\tusername = x-access-token\n",
        ),
        ("token-only", config, "[credential]\n\thelper = token\n"),
        ("password-only", config, "[credential]\n\tpassword = x\n"),
    ]);
}

#[test]
fn a_credential_under_home_is_found_even_when_cargo_home_is_elsewhere() {
    let repo = private_checkout("decide-credential-elsewhere", GIT_LOCK);
    repo.write_unstaged(
        "home/.cargo/git/db/shared-1/.git-credentials",
        "https://x-access-token:ghs_secret@github.com\n",
    );
    let api = serve_body(200, PRIVATE).unwrap_or_else(|e| unreachable!("{e}"));
    let (output, written) = decide_in(&repo, Some(&api), Some(TOKEN), Some("elsewhere"));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=false\n", "{output:?}");
}

#[test]
fn credential_files_outside_the_saved_sources_paths_do_not_matter() {
    sources_save_goes_on(&[
        (
            "home-git-credentials",
            ".git-credentials",
            "https://x-access-token:ghs_secret@github.com\n",
        ),
        (
            "home-netrc",
            ".netrc",
            "machine github.com login x password ghs_secret\n",
        ),
        (
            "cargo-credentials",
            ".cargo/credentials.toml",
            "[registry]\ntoken = \"secret\"\n",
        ),
    ]);
}

#[test]
fn plain_git_data_inside_the_saved_sources_paths_is_saved() {
    sources_save_goes_on(&[(
        "plain-git-config",
        ".cargo/git/db/shared-1/config",
        "[core]\n\tbare = true\n[remote \"origin\"]\n\turl = https://github.com/acme/shared-lib\n",
    )]);
}

#[test]
fn sources_are_not_saved_when_home_is_unset_and_the_saved_paths_cannot_be_checked() {
    let repo = private_checkout("decide-no-home", GIT_LOCK);
    let api = serve_body(200, PRIVATE).unwrap_or_else(|e| unreachable!("{e}"));
    let (output, written) = decide_without_home(&repo, Some(&api), Some(TOKEN), Some("cargo-home"));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=false\n", "{output:?}");
}
