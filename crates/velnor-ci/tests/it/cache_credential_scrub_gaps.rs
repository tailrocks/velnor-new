// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache_credential_scrub::{sources_save_goes_on, sources_save_stops};

const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
const CONFIG: &str = ".cargo/git/db/shared-1/config";
const TOKEN: &str = concat!("ghp_", "0123456789abcdefghijklmnopqrstuvwxyzAB");

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

fn commented(text: &str) -> String {
    let lines: Vec<String> = text
        .lines()
        .map(|line| format!("# {}", line.trim_start_matches("# ")))
        .collect();
    format!("[core]\n\tbare = true\n{}\n", lines.join("\n"))
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
            "config-encoded-classic",
            CONFIG,
            &commented(&format!("{SHA}\t\tbranch 'main' {encoded}")),
        ),
        (
            "config-encoded-fine-grained",
            CONFIG,
            &commented(&format!("{SHA}\t\tbranch 'main' {fine_grained}")),
        ),
        (
            "config-encoded-url",
            CONFIG,
            &commented(&format!("{SHA}\t\tbranch 'main' of {encoded_url}")),
        ),
    ]);
}

#[test]
fn a_basic_authorization_header_in_a_config_comment_stops_the_sources_save() {
    let user = "x-access-token";
    let token = format!("{}{}", "ghs_", "secret1234567890");
    let basic = encode_basic(format!("{user}:{token}").as_bytes());
    sources_save_stops(&[
        (
            "config-header",
            CONFIG,
            &commented(&format!(
                "{SHA}\t\tbranch 'main' Authorization: Basic {basic}"
            )),
        ),
        (
            "config-short-basic-header",
            CONFIG,
            &commented(&format!(
                "{SHA}\t\tbranch 'main' Authorization: Basic {}",
                encode_basic(b"user:pass")
            )),
        ),
    ]);
}

#[test]
fn prose_about_a_basic_authorization_header_is_saved() {
    sources_save_goes_on(&[
        (
            "placeholder-token",
            CONFIG,
            &commented("send Authorization: Basic <base64 of user:token> when asked"),
        ),
        (
            "variable-reference",
            CONFIG,
            &commented("curl -H \"Authorization: Basic $ENCODED\" https://example.org"),
        ),
        (
            "word-after-basic",
            CONFIG,
            &commented("the Authorization: Basic header is never written here"),
        ),
        (
            "bearer-prose",
            CONFIG,
            &commented("an authorization header like Bearer is configured elsewhere"),
        ),
        (
            "basic-auth-prose",
            CONFIG,
            &commented("Authorization: Basic authentication is required"),
        ),
        (
            "benign-four-times-encoded-space",
            CONFIG,
            &commented("%25252520"),
        ),
    ]);
}

#[test]
fn a_basic_authorization_header_with_non_ascii_credentials_stops_the_sources_save() {
    sources_save_stops(&[(
        "config-utf8-password",
        CONFIG,
        &commented(&format!(
            "{SHA}\t\tbranch 'main' Authorization: Basic {}",
            encode_basic(b"user:p\xc3\xa4ss")
        )),
    )]);
}

#[test]
fn a_credential_in_a_comment_of_a_saved_git_config_stops_the_sources_save() {
    sources_save_stops(&[
        (
            "config-comment-encoded-token",
            CONFIG,
            &commented(&TOKEN.replace('_', "%5F")),
        ),
        (
            "config-comment-basic-header",
            CONFIG,
            &commented("Authorization: Basic dXNlcjpwYXNz"),
        ),
    ]);
}

#[test]
fn a_form_encoded_basic_header_stops_the_sources_save() {
    sources_save_stops(&[(
        "config-form-encoded-basic",
        CONFIG,
        &commented(&format!(
            "{SHA}\t\tbranch 'main' Authorization%3A+Basic+dXNlcjpwYXNz"
        )),
    )]);
}

#[test]
fn a_basic_payload_with_control_bytes_stops_the_sources_save() {
    let line = |credentials: &[u8]| {
        commented(&format!(
            "{SHA}\t\tbranch 'main' Authorization: Basic {}",
            encode_basic(credentials)
        ))
    };
    sources_save_stops(&[
        (
            "config-base64-with-a-tab",
            CONFIG,
            &commented(&format!(
                "{SHA}\t\tbranch 'main' Authorization: Basic dXNlcjpwYQlzcw=="
            )),
        ),
        (
            "config-base64-with-a-trailing-newline",
            CONFIG,
            &line(b"user:pass\n"),
        ),
        (
            "config-base64-with-a-delete-byte",
            CONFIG,
            &line(b"user:pa\x7fss"),
        ),
    ]);
}

#[test]
fn a_double_quoted_basic_payload_stops_the_sources_save() {
    sources_save_stops(&[
        (
            "config-double-quoted-basic",
            CONFIG,
            &commented("Authorization: Basic \"dXNlcjpwYXNz\""),
        ),
        (
            "config-double-quoted-basic-after-a-space",
            CONFIG,
            &commented(&format!(
                "{SHA}\t\tbranch 'main' Authorization: Basic  \" {}\"",
                encode_basic(b"user:pass")
            )),
        ),
    ]);
}

#[test]
fn a_credential_split_by_shell_quotes_in_a_config_comment_stops_the_sources_save() {
    let basic = encode_basic(b"user:pass");
    let (head, tail) = basic.split_at(4);
    let split_token = TOKEN.replacen("0123456789", "01234\"\"56789", 1);
    sources_save_stops(&[
        (
            "config-basic-split-by-double-quotes",
            CONFIG,
            &commented(&format!("Authorization: Basic {head}\"\"{tail}")),
        ),
        (
            "config-basic-split-by-single-quotes",
            CONFIG,
            &commented(&format!("Authorization: Basic {head}''{tail}")),
        ),
        (
            "config-token-split-by-quotes",
            CONFIG,
            &commented(&split_token),
        ),
    ]);
}

#[test]
fn a_form_encoded_basic_payload_with_an_encoded_plus_stops_the_sources_save() {
    let line = |text: &str| commented(&format!("{SHA}\t\tbranch 'main' {text}"));
    sources_save_stops(&[
        (
            "config-form-plus-in-payload",
            CONFIG,
            &commented("Authorization%3A+Basic+8J%2BYgDpwYXNz"),
        ),
        (
            "config-form-plus-in-payload-after-a-sha",
            CONFIG,
            &line("Authorization%3A+Basic+8J%2BYgDpwYXNz"),
        ),
        (
            "config-percent-header-plus-payload",
            CONFIG,
            &line("Authorization%3A%20Basic%20Aw%2BYgDpwYXNz"),
        ),
        (
            "config-two-layers-plus-in-payload",
            CONFIG,
            &line("Authorization%253A%2BBasic%2B8J%252BYgDpwYXNz"),
        ),
        (
            "config-plain-header-encoded-plus",
            CONFIG,
            &line("Authorization: Basic 8J%2BYgDpwYXNz"),
        ),
        (
            "config-mixed-layers-plus-in-payload",
            CONFIG,
            &line("Authorization%3A+Basic+8J%252BYgDpwYXNz"),
        ),
    ]);
}
