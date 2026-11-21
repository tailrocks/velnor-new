// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache_credential_scrub::{sources_save_goes_on, sources_save_stops};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const HOOK: &str = ".cargo/git/db/shared-1/hooks/post-fetch";
const FETCH_HEAD: &str = ".cargo/git/db/shared-1/FETCH_HEAD";
const TOKEN: &str = "ghp_0123456789abcdefghijklmnopqrstuvwxyzAB";

fn encode_basic(credentials: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    credentials
        .chunks(3)
        .flat_map(|chunk| {
            let bits = chunk.iter().enumerate().fold(0u32, |acc, (i, byte)| {
                acc | (u32::from(*byte) << (16 - 8 * i))
            });
            (0..4).map(move |i| {
                if i <= chunk.len() {
                    char::from(ALPHABET[((bits >> (18 - 6 * i)) & 63) as usize])
                } else {
                    '='
                }
            })
        })
        .collect()
}

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
    ]);
}

#[test]
fn a_percent_encoded_token_prefix_in_plain_text_stops_the_sources_save() {
    let encoded = TOKEN.replace('_', "%5F");
    let fine_grained = "github%5Fpat%5F0123456789abcdefghijklmnopqrstuvwxyzAB";
    let user = "x-access-token";
    let pass = "secret";
    let encoded_url = format!("https%3A%2F%2F{user}:{pass}%40github.com/acme/shared-lib");
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
    let user = "x-access-token";
    let token = format!("{}{}", "ghs_", "secret1234567890");
    let basic = encode_basic(format!("{user}:{token}").as_bytes());
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
            &format!(
                "curl -H 'Authorization: Basic {}' https://example.org\n",
                encode_basic(b"user:pass")
            ),
        ),
        (
            "fetch-head-short-basic-header",
            FETCH_HEAD,
            &format!(
                "{SHA}\t\tbranch 'main' Authorization: Basic {}\n",
                encode_basic(b"user:pass")
            ),
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

#[test]
fn a_basic_authorization_header_with_non_ascii_credentials_stops_the_sources_save() {
    sources_save_stops(&[
        (
            "hook-utf8-password",
            HOOK,
            &format!(
                "curl -H 'Authorization: Basic {}' https://example.org\n",
                encode_basic(b"user:p\xc3\xa4ss")
            ),
        ),
        (
            "hook-utf8-user",
            HOOK,
            &format!(
                "curl -H 'Authorization: Basic {}' https://example.org\n",
                encode_basic(b"\xc3\xbcser:pass")
            ),
        ),
        (
            "hook-invalid-utf8-password",
            HOOK,
            &format!(
                "curl -H 'Authorization: Basic {}' https://example.org\n",
                encode_basic(b"user:p\xe4ss")
            ),
        ),
        (
            "hook-invalid-utf8-user",
            HOOK,
            &format!(
                "curl -H 'Authorization: Basic {}' https://example.org\n",
                encode_basic(b"us\xe9r:pass")
            ),
        ),
        (
            "hook-invalid-utf8-bytes-only",
            HOOK,
            &format!(
                "curl -H 'Authorization: Basic {}' https://example.org\n",
                encode_basic(b"user:p\xff\xfess")
            ),
        ),
        (
            "fetch-head-utf8-password",
            FETCH_HEAD,
            &format!(
                "{SHA}\t\tbranch 'main' Authorization: Basic {}\n",
                encode_basic(b"user:p\xc3\xa4ss")
            ),
        ),
    ]);
}
