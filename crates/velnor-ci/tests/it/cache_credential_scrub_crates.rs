// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache_credential_scrub::{sources_save_goes_on, sources_save_stops};

const INDEX_ENTRY: &str = "\0\0\0\u{1}1.0\0{\"name\":\"crate\",\"vers\":\"1.0.0\"}\0";
const INDEX: &str = ".cargo/registry/index/index.crates.io-x/.cache";

#[test]
fn sparse_index_entries_for_crates_named_like_credential_files_do_not_stop_the_sources_save() {
    let config = format!("{INDEX}/co/nf/config");
    let credentials = format!("{INDEX}/cr/ed/credentials");
    let hosts = format!("{INDEX}/ho/st/hosts");
    sources_save_goes_on(&[
        ("index-config-crate", &config, INDEX_ENTRY),
        ("index-credentials-crate", &credentials, INDEX_ENTRY),
        ("index-hosts-crate", &hosts, INDEX_ENTRY),
        (
            "cache-config-crate",
            ".cargo/registry/cache/index.crates.io-x/config",
            INDEX_ENTRY,
        ),
        (
            "cache-credentials-crate",
            ".cargo/registry/cache/index.crates.io-x/credentials",
            INDEX_ENTRY,
        ),
    ]);
}

#[test]
fn a_credential_file_inside_git_metadata_still_stops_the_sources_save() {
    sources_save_stops(&[
        (
            "db-credentials",
            ".cargo/git/db/shared-1/credentials",
            "https://x-access-token:ghs_secret@github.com\n",
        ),
        (
            "db-binary-config",
            ".cargo/git/db/shared-1/config",
            "\0\0binary\0",
        ),
        (
            "db-token-config",
            ".cargo/git/db/shared-1/config",
            "[remote \"origin\"]\n\turl = https://x-access-token:ghs_secret@github.com/acme/lib\n",
        ),
        (
            "legacy-index-git-config",
            ".cargo/registry/index/github.com-x/.git/config",
            "[remote \"origin\"]\n\turl = https://x-access-token:ghs_secret@github.com/acme/index\n",
        ),
        (
            "legacy-index-git-credentials",
            ".cargo/registry/index/github.com-x/.git/credentials",
            "https://x-access-token:ghs_secret@github.com\n",
        ),
        (
            "index-dotted-credential-file",
            ".cargo/registry/index/index.crates.io-x/.netrc",
            "machine github.com login x password ghs_secret\n",
        ),
    ]);
}
