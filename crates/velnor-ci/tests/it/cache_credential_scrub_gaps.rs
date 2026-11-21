// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache_credential_scrub::{sources_save_goes_on, sources_save_stops};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const HOOK: &str = ".cargo/git/db/shared-1/hooks/post-fetch";
const FETCH_HEAD: &str = ".cargo/git/db/shared-1/FETCH_HEAD";
const TOKEN: &str = "ghp_0123456789abcdefghijklmnopqrstuvwxyzAB";

#[test]
fn a_ref_named_hooks_is_not_the_hook_tree_and_is_saved() {
    sources_save_goes_on(&[
        (
            "remote-ref-named-hooks",
            ".cargo/git/db/shared-1/refs/remotes/origin/hooks/main",
            SHA,
        ),
        (
            "head-ref-named-hooks",
            ".cargo/git/db/shared-1/refs/heads/hooks/main",
            SHA,
        ),
        (
            "ref-named-hooks-in-a-registry-index-repository",
            ".cargo/registry/index/index.crates.io-1/.git/refs/heads/hooks/main",
            SHA,
        ),
        (
            "ref-named-hooks-in-a-submodule-git-directory",
            ".cargo/git/db/shared-1/modules/vendor/refs/heads/hooks/main",
            SHA,
        ),
        (
            "remote-ref-named-hooks-in-a-nested-submodule-git-directory",
            ".cargo/git/db/shared-1/modules/vendor/modules/deep/refs/remotes/origin/hooks/main",
            SHA,
        ),
    ]);
}

#[test]
fn the_hook_tree_of_every_repository_is_still_scanned() {
    sources_save_stops(&[
        ("bare-repository-hook", HOOK, &format!("TOKEN={SHA}\n")),
        (
            "registry-index-repository-hook",
            ".cargo/registry/index/index.crates.io-1/.git/hooks/post-fetch",
            &format!("TOKEN={SHA}\n"),
        ),
        (
            "submodule-repository-hook",
            ".cargo/git/db/shared-1/modules/vendor/hooks/post-fetch",
            &format!("TOKEN={SHA}\n"),
        ),
        (
            "nested-submodule-repository-hook",
            ".cargo/git/db/shared-1/modules/vendor/deep/hooks/post-fetch",
            &format!("TOKEN={SHA}\n"),
        ),
    ]);
}

#[test]
fn a_percent_encoded_token_prefix_in_plain_text_stops_the_sources_save() {
    let encoded = TOKEN.replace('_', "%5F");
    let fine_grained = "github%5Fpat%5F0123456789abcdefghijklmnopqrstuvwxyzAB";
    let encoded_url = "https%3A%2F%2Fx-access-token:secret%40github.com/acme/shared-lib";
    sources_save_stops(&[
        ("hook-encoded-classic", HOOK, &format!("echo {encoded}\n")),
        (
            "hook-encoded-fine-grained",
            HOOK,
            &format!("echo {fine_grained}\n"),
        ),
        ("hook-encoded-url", HOOK, &format!("# {encoded_url}\n")),
        (
            "fetch-head-encoded-classic",
            FETCH_HEAD,
            &format!("{SHA}\t\tbranch 'main' {encoded}\n"),
        ),
        (
            "fetch-head-encoded-fine-grained",
            FETCH_HEAD,
            &format!("{SHA}\t\tbranch 'main' {fine_grained}\n"),
        ),
    ]);
}

#[test]
fn a_token_split_by_the_shell_in_a_hook_stops_the_sources_save() {
    sources_save_stops(&[
        (
            "line-continuation",
            HOOK,
            "echo ghp_0123456789\\\nabcdefghijklmnopqrstuvwxyzAB\n",
        ),
        (
            "line-continuation-crlf",
            HOOK,
            "echo ghp_0123456789\\\r\nabcdefghijklmnopqrstuvwxyzAB\n",
        ),
        (
            "double-quote-concatenation",
            HOOK,
            "echo ghp_0123456789\"abcdefghijklmnopqrstuvwxyzAB\"\n",
        ),
        (
            "single-quote-concatenation",
            HOOK,
            "echo ghp_0123456789'abcdefghijklmnopqrstuvwxyzAB'\n",
        ),
        (
            "dollar-single-quote-concatenation",
            HOOK,
            "echo ghp_0123456789$'abcdefghijklmnopqrstuvwxyzAB'\n",
        ),
        (
            "dollar-double-quote-concatenation",
            HOOK,
            "echo ghp_0123456789$\"abcdefghijklmnopqrstuvwxyzAB\"\n",
        ),
        (
            "empty-quotes-inside-the-prefix",
            HOOK,
            "echo gh\"\"p_0123456789abcdefghijklmnopqrstuvwxyzAB\n",
        ),
        (
            "hex-token-split-by-continuation",
            HOOK,
            "TOKEN=0123456789abcdef0123\\\n456789abcdef01234567\n",
        ),
        (
            "fine-grained-split-by-continuation",
            HOOK,
            "echo github_pat_0123456789\\\nabcdefghijklmnopqrstuvwxyzAB\n",
        ),
    ]);
}

#[test]
fn a_basic_authorization_header_in_a_hook_or_fetch_head_stops_the_sources_save() {
    let basic = "eC1hY2Nlc3MtdG9rZW46Z2hzX3NlY3JldDEyMzQ1Njc4OTA=";
    sources_save_stops(&[
        (
            "hook-extraheader",
            HOOK,
            &format!("git -c http.extraheader=\"AUTHORIZATION: basic {basic}\" fetch\n"),
        ),
        (
            "hook-curl-header",
            HOOK,
            &format!("curl -H 'Authorization: Basic {basic}' https://example.org\n"),
        ),
        (
            "hook-proxy-header",
            HOOK,
            &format!("curl -H 'Proxy-Authorization:Basic {basic}' https://example.org\n"),
        ),
        (
            "hook-encoded-space",
            HOOK,
            &format!("# Authorization:%20Basic%20{basic}\n"),
        ),
        (
            "fetch-head-header",
            FETCH_HEAD,
            &format!("{SHA}\t\tbranch 'main' Authorization: Basic {basic}\n"),
        ),
        (
            "hook-short-basic-header",
            HOOK,
            "curl -H 'Authorization: Basic dXNlcjpwYXNz' https://example.org\n",
        ),
        (
            "fetch-head-short-basic-header",
            FETCH_HEAD,
            &format!("{SHA}\t\tbranch 'main' Authorization: Basic dXNlcjpwYXNz\n"),
        ),
    ]);
}

#[test]
fn prose_about_a_basic_authorization_header_is_saved() {
    sources_save_goes_on(&[
        (
            "placeholder-token",
            HOOK,
            "# send Authorization: Basic <base64 of user:token> when asked\n",
        ),
        (
            "variable-reference",
            HOOK,
            "curl -H \"Authorization: Basic $ENCODED\" https://example.org\n",
        ),
        (
            "word-after-basic",
            HOOK,
            "# the Authorization: Basic header is never written here\n",
        ),
        (
            "bearer-prose",
            HOOK,
            "# an authorization header like Bearer is configured elsewhere\n",
        ),
        (
            "basic-auth-prose",
            HOOK,
            "# Authorization: Basic authentication is required\n",
        ),
        ("benign-four-times-encoded-space", FETCH_HEAD, "%25252520\n"),
    ]);
}
