// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Output;

use crate::cache_private_repo::{
    GIT_LOCK, decide_in, decide_with, decide_without_home, private_checkout,
};

fn saved_sources_with(name: &str, relative: &str, content: &str) -> (Output, String) {
    let repo = private_checkout(name, GIT_LOCK);
    repo.write_unstaged(&format!("home/{relative}"), content);
    decide_with(&repo, Some("true"))
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
fn a_credential_under_home_is_found_even_when_cargo_home_is_elsewhere() {
    let repo = private_checkout("decide-credential-elsewhere", GIT_LOCK);
    repo.write_unstaged(
        "home/.cargo/git/db/shared-1/.git-credentials",
        "https://x-access-token:ghs_secret@github.com\n",
    );
    let (output, written) = decide_in(&repo, Some("true"), Some("elsewhere"));
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
    let (output, written) = decide_without_home(&repo, Some("true"), Some("cargo-home"));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=false\n", "{output:?}");
}

#[test]
fn credential_file_bare_credentials_alone_stops_the_sources_save() {
    sources_save_stops(&[(
        "credential_file_bare_credentials",
        ".cargo/git/db/shared-1/credentials",
        "x\n",
    )]);
}

#[test]
fn credential_file_git_credentials_alone_stops_the_sources_save() {
    sources_save_stops(&[(
        "credential_file_git_credentials",
        ".cargo/git/db/shared-1/.git-credentials",
        "x\n",
    )]);
}

#[test]
fn credential_file_netrc_alone_stops_the_sources_save() {
    sources_save_stops(&[(
        "credential_file_netrc",
        ".cargo/git/db/shared-1/.netrc",
        "x\n",
    )]);
}

#[test]
fn credential_file_netrc_windows_alone_stops_the_sources_save() {
    sources_save_stops(&[(
        "credential_file_netrc_windows",
        ".cargo/git/db/shared-1/_netrc",
        "x\n",
    )]);
}

#[test]
fn credential_file_credentials_toml_alone_stops_the_sources_save() {
    sources_save_stops(&[(
        "credential_file_credentials_toml",
        ".cargo/git/db/shared-1/credentials.toml",
        "x\n",
    )]);
}

#[test]
fn credential_file_hosts_yml_alone_stops_the_sources_save() {
    sources_save_stops(&[(
        "credential_file_hosts_yml",
        ".cargo/git/db/shared-1/hosts.yml",
        "x\n",
    )]);
}

#[test]
fn userinfo_https_alone_stops_the_sources_save() {
    sources_save_stops(&[(
        "userinfo_https",
        ".cargo/git/db/shared-1/config",
        "[remote \"origin\"]\n\turl = https://deploy@github.com/acme/shared-lib\n",
    )]);
}

#[test]
fn userinfo_http_alone_stops_the_sources_save() {
    sources_save_stops(&[(
        "userinfo_http",
        ".cargo/git/db/shared-1/config",
        "[remote \"origin\"]\n\turl = http://deploy@github.com/acme/shared-lib\n",
    )]);
}

#[test]
fn extraheader_alone_stops_the_sources_save() {
    sources_save_stops(&[(
        "extraheader",
        ".cargo/git/db/shared-1/config",
        "[http]\n\textraheader = x\n",
    )]);
}

#[test]
fn authorization_alone_stops_the_sources_save() {
    sources_save_stops(&[(
        "authorization",
        ".cargo/git/db/shared-1/config",
        "[http]\n\theader = Authorization: x\n",
    )]);
}

#[test]
fn password_alone_stops_the_sources_save() {
    sources_save_stops(&[(
        "password",
        ".cargo/git/db/shared-1/config",
        "[credential]\n\tpassword = x\n",
    )]);
}

#[test]
fn token_alone_stops_the_sources_save() {
    sources_save_stops(&[(
        "token",
        ".cargo/git/db/shared-1/config",
        "[credential]\n\thelper = token\n",
    )]);
}

#[test]
fn a_userinfo_url_split_by_a_git_config_continuation_stops_the_sources_save() {
    sources_save_stops(&[(
        "continued-url",
        ".cargo/git/db/shared-1/config",
        "[remote \"origin\"]\n\turl = https://deploy:sec\\\nret@github.com/acme/shared-lib\n",
    )]);
}

#[test]
fn a_git_config_that_delegates_credentials_or_changes_transport_stops_the_sources_save() {
    sources_save_stops(&[
        (
            "credential-store-file",
            ".cargo/git/db/shared-1/config",
            "[credential]\n\thelper = store --file=/home/runner/.cargo/git/db/shared-1/opaque\n",
        ),
        (
            "proxy-without-verification",
            ".cargo/git/db/shared-1/config",
            "[http]\n\tproxy = http://attacker.invalid:3128\n\tsslVerify = false\n",
        ),
        (
            "ssh-command",
            ".cargo/git/db/shared-1/config",
            "[core]\n\tbare = true\n\tsshCommand = ssh -o ProxyCommand=evil\n",
        ),
        (
            "include",
            ".cargo/git/db/shared-1/config",
            "[include]\n\tpath = /tmp/elsewhere\n",
        ),
        (
            "insteadof",
            ".cargo/git/db/shared-1/config",
            "[url \"https://attacker.invalid/\"]\n\tinsteadOf = https://github.com/\n",
        ),
        (
            "upload-pack",
            ".cargo/git/db/shared-1/config",
            "[remote \"origin\"]\n\turl = https://github.com/acme/shared-lib\n\tuploadpack = evil\n",
        ),
        (
            "same-line-key",
            ".cargo/git/db/shared-1/config",
            "[http] proxy = http://attacker.invalid:3128\n",
        ),
        (
            "worktree-config",
            ".cargo/git/db/shared-1/config.worktree",
            "[http]\n\tproxy = http://attacker.invalid:3128\n",
        ),
    ]);
}
