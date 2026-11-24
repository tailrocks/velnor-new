// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache_credential_scrub::{sources_save_goes_on, sources_save_stops};
use crate::cache_private_repo::{GIT_LOCK, decide_with, private_checkout};

const DATABASE: &str = "home/.cargo/git/db/shared-1";
const CREDENTIAL_URL: &str = concat!(
    "https://x-access-token:",
    "ghs_",
    "secret@github.com/acme/shared-lib"
);
const HEX_TOKEN: &str = concat!("0123456789abcdef0123", "456789abcdef01234567");
const NETRC: &str = concat!("machine github.com login x password ", "ghs_secret\n");

#[test]
fn legacy_remote_files_of_a_git_database_are_left_out_of_the_saved_sources() {
    let repo = private_checkout("decide-unscanned-remotes", GIT_LOCK);
    for (file, content) in [
        ("HEAD", "ref: refs/heads/main\n".to_owned()),
        ("config", "[core]\n\tbare = true\n".to_owned()),
        ("objects/info/packs", "P pack-1.pack\n".to_owned()),
        ("remotes/origin", format!("URL: {CREDENTIAL_URL}\n")),
        ("branches/main", format!("{CREDENTIAL_URL}#main\n")),
        (
            "objects/info/http-alternates",
            format!("{CREDENTIAL_URL}/objects\n"),
        ),
    ] {
        repo.write_unstaged(&format!("{DATABASE}/{file}"), &content);
    }
    let (output, written) = decide_with(&repo, Some("true"));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=true\n", "{output:?}");
    let git = repo.root().join(DATABASE);
    for gone in ["remotes", "branches", "objects/info/http-alternates"] {
        assert!(!git.join(gone).exists(), "{gone}");
    }
    for kept in ["HEAD", "config", "objects/info/packs"] {
        assert!(git.join(kept).is_file(), "{kept}");
    }
}

#[test]
fn a_credential_in_a_registry_config_stops_the_sources_save() {
    sources_save_stops(&[
        (
            "registry-dl-userinfo",
            ".cargo/registry/index/registry.example-1/config.json",
            concat!(
                "{\"dl\": \"https://user:",
                "secret@registry.example/crates\", \"api\": \"https://registry.example\"}\n"
            ),
        ),
        (
            "registry-api-token",
            ".cargo/registry/index/registry.example-1/config.json",
            concat!(
                "{\"dl\": \"https://registry.example/crates\", \"api\": \"https://registry.example/ghp_",
                "0123456789abcdefghijklmnopqrstuvwxyzAB\"}\n"
            ),
        ),
    ]);
    sources_save_goes_on(&[
        (
            "crates-io-config",
            ".cargo/registry/index/index.crates.io-1/config.json",
            "{\n  \"dl\": \"https://static.crates.io/crates\",\n  \"api\": \"https://crates.io\"\n}",
        ),
        (
            "templated-config",
            ".cargo/registry/index/registry.example-1/config.json",
            "{\"dl\": \"https://registry.example/{crate}/{version}/download\", \"api\": \"https://registry.example\", \"auth-required\": true}\n",
        ),
    ]);
}

#[test]
fn an_escaped_or_database_shaped_registry_config_still_stops_the_sources_save() {
    sources_save_stops(&[
        (
            "registry-dl-escaped-slashes",
            ".cargo/registry/index/registry.example-1/config.json",
            concat!(
                "{\"dl\": \"https:\\/\\/user:",
                "secret@registry.example\\/crates\"}\n"
            ),
        ),
        (
            "registry-dl-escaped-at",
            ".cargo/registry/index/registry.example-1/config.json",
            concat!(
                "{\"dl\": \"https://user:",
                "secret\\u0040registry.example/crates\"}\n"
            ),
        ),
    ]);
    let repo = private_checkout("decide-unscanned-registry-shaped-like-a-database", GIT_LOCK);
    for (file, content) in [
        ("HEAD", "ref: refs/heads/main\n"),
        ("config", "[core]\n\tbare = true\n"),
        (
            "config.json",
            concat!(
                "{\"dl\": \"https://user:",
                "secret@registry.example/crates\"}\n"
            ),
        ),
    ] {
        repo.write_unstaged(
            &format!("home/.cargo/registry/index/registry.example-1/{file}"),
            content,
        );
    }
    let (output, written) = decide_with(&repo, Some("true"));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=false\n", "{output:?}");
}

#[test]
fn a_bare_hex_token_anywhere_in_a_saved_git_config_stops_the_sources_save() {
    let configs = [
        format!("[core]\n\tbare = true\n# {HEX_TOKEN}\n"),
        format!("; token {HEX_TOKEN}\n[core]\n\tbare = true\n"),
        format!("[core]\n\tbare = {HEX_TOKEN}\n"),
        format!("[remote \"{HEX_TOKEN}\"]\n\turl = https://github.com/acme/shared-lib\n"),
    ];
    let labels = ["hash-comment", "semicolon-comment", "value", "remote-name"];
    let cases: Vec<(&str, &str, &str)> = labels
        .iter()
        .zip(&configs)
        .map(|(label, config)| (*label, ".cargo/git/db/shared-1/config", config.as_str()))
        .collect();
    sources_save_stops(&cases);
}

#[test]
fn only_a_sparse_index_entry_at_its_own_path_may_carry_a_credential_file_name() {
    sources_save_goes_on(&[(
        "netrc-index-entry",
        ".cargo/registry/index/registry.example-1/.cache/_n/et/_netrc",
        "\u{3}\u{2}\x00\x00\x00etag\x001.0.0\x00{\"name\":\"_netrc\",\"vers\":\"1.0.0\"}\x00",
    )]);
    sources_save_stops(&[
        (
            "netrc-beside-the-index",
            ".cargo/registry/index/registry.example-1/_netrc",
            NETRC,
        ),
        (
            "netrc-at-another-index-path",
            ".cargo/registry/index/registry.example-1/.cache/_n/_netrc",
            NETRC,
        ),
        (
            "dot-netrc-at-an-index-path",
            ".cargo/registry/index/registry.example-1/.cache/.n/et/.netrc",
            NETRC,
        ),
        (
            "netrc-under-a-nested-index",
            ".cargo/registry/index/registry.example-1/nested/index/fake/.cache/_n/et/_netrc",
            NETRC,
        ),
        (
            "netrc-under-an-index-in-the-crate-cache",
            ".cargo/registry/cache/index/fake/.cache/_n/et/_netrc",
            NETRC,
        ),
        (
            "netrc-index-path-in-a-git-database",
            ".cargo/git/db/shared-1/.cache/_n/et/_netrc",
            NETRC,
        ),
    ]);
}

#[test]
fn a_refused_admission_leaves_the_cargo_home_untouched() {
    let repo = private_checkout("decide-unscanned-refused", GIT_LOCK);
    repo.write_unstaged(&format!("{DATABASE}/HEAD"), "ref: refs/heads/main\n");
    repo.write_unstaged(&format!("{DATABASE}/hooks/post-fetch"), "exit 0\n");
    let (output, written) = decide_with(&repo, Some("false"));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=false\nsources=false\n", "{output:?}");
    assert!(
        repo.root()
            .join(DATABASE)
            .join("hooks/post-fetch")
            .is_file()
    );
}

#[test]
fn a_confirmed_private_repository_saves_the_base_caches_for_an_http_git_source() {
    let plain = GIT_LOCK.replace("git+https://github.com/", "git+http://ghe.example/");
    let repo = private_checkout("decide-unscanned-http", &plain);
    let (output, written) = decide_with(&repo, Some("true"));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=true\nsources=true\n", "{output:?}");
    let credentialed = GIT_LOCK.replace(
        "git+https://github.com/",
        concat!("git+http://user:", "secret@ghe.example/"),
    );
    let repo = private_checkout("decide-unscanned-http-userinfo", &credentialed);
    let (output, written) = decide_with(&repo, Some("true"));
    assert!(output.status.success(), "{output:?}");
    assert_eq!(written, "save=false\nsources=false\n", "{output:?}");
}
