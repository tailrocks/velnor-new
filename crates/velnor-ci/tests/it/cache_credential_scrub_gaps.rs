// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache_credential_scrub::{sources_save_goes_on, sources_save_stops};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
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
fn a_percent_encoded_token_prefix_in_plain_text_stops_the_sources_save() {
    let encoded = TOKEN.replace('_', "%5F");
    let fine_grained = "github%5Fpat%5F0123456789abcdefghijklmnopqrstuvwxyzAB";
    let user = "x-access-token";
    let pass = "secret";
    let encoded_url = format!("https%3A%2F%2F{user}:{pass}%40github.com/acme/shared-lib");
    sources_save_stops(&[
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
        (
            "fetch-head-encoded-url",
            FETCH_HEAD,
            &format!("{SHA}\t\tbranch 'main' of {encoded_url}\n"),
        ),
    ]);
}

#[test]
fn a_basic_authorization_header_in_fetch_head_stops_the_sources_save() {
    let user = "x-access-token";
    let token = format!("{}{}", "ghs_", "secret1234567890");
    let basic = encode_basic(format!("{user}:{token}").as_bytes());
    sources_save_stops(&[
        (
            "fetch-head-header",
            FETCH_HEAD,
            &format!("{SHA}\t\tbranch 'main' Authorization: Basic {basic}\n"),
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
            FETCH_HEAD,
            "# send Authorization: Basic <base64 of user:token> when asked\n",
        ),
        (
            "variable-reference",
            FETCH_HEAD,
            "curl -H \"Authorization: Basic $ENCODED\" https://example.org\n",
        ),
        (
            "word-after-basic",
            FETCH_HEAD,
            "# the Authorization: Basic header is never written here\n",
        ),
        (
            "bearer-prose",
            FETCH_HEAD,
            "# an authorization header like Bearer is configured elsewhere\n",
        ),
        (
            "basic-auth-prose",
            FETCH_HEAD,
            "# Authorization: Basic authentication is required\n",
        ),
        ("benign-four-times-encoded-space", FETCH_HEAD, "%25252520\n"),
    ]);
}

#[test]
fn a_basic_authorization_header_with_non_ascii_credentials_stops_the_sources_save() {
    sources_save_stops(&[(
        "fetch-head-utf8-password",
        FETCH_HEAD,
        &format!(
            "{SHA}\t\tbranch 'main' Authorization: Basic {}\n",
            encode_basic(b"user:p\xc3\xa4ss")
        ),
    )]);
}

#[test]
fn a_credential_in_a_comment_of_a_saved_git_config_stops_the_sources_save() {
    let config = ".cargo/git/db/shared-1/config";
    sources_save_stops(&[
        (
            "config-comment-encoded-token",
            config,
            &format!("[core]\n\tbare = true\n# {}\n", TOKEN.replace('_', "%5F")),
        ),
        (
            "config-comment-basic-header",
            config,
            "[core]\n\tbare = true\n# Authorization: Basic dXNlcjpwYXNz\n",
        ),
    ]);
}

#[test]
fn a_form_encoded_basic_header_stops_the_sources_save() {
    sources_save_stops(&[(
        "fetch-head-form-encoded-basic",
        FETCH_HEAD,
        &format!("{SHA}\t\tbranch 'main' Authorization%3A+Basic+dXNlcjpwYXNz\n"),
    )]);
}

#[test]
fn a_basic_payload_with_control_bytes_stops_the_sources_save() {
    sources_save_stops(&[(
        "fetch-head-base64-with-a-tab",
        FETCH_HEAD,
        &format!("{SHA}\t\tbranch 'main' Authorization: Basic dXNlcjpwYQlzcw==\n"),
    )]);
}
