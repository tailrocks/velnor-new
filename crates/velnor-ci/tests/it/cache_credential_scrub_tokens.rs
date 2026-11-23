// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache_credential_scrub::{sources_save_goes_on, sources_save_stops};

const FAKE_BODY: &str = "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8";
const CONFIG: &str = ".cargo/git/db/shared-1/config";

fn as_cases(owned: &[(String, String, String)]) -> Vec<(&str, &str, &str)> {
    owned
        .iter()
        .map(|(label, relative, content)| (label.as_str(), relative.as_str(), content.as_str()))
        .collect()
}

#[test]
fn a_full_token_glued_to_other_characters_stops_the_sources_save() {
    let sha = "0123456789abcdef0123456789abcdef01234567";
    let forms: Vec<(&str, String)> = vec![
        ("letter-before-ghp", format!("xghp_{FAKE_BODY}")),
        ("digit-before-ghp", format!("1ghp_{FAKE_BODY}")),
        ("escaped-newline-before-ghp", format!("a\\nghp_{FAKE_BODY}")),
        ("escaped-tab-before-ghp", format!("a\\tghp_{FAKE_BODY}")),
        (
            "url-encoded-newline-before-ghp",
            format!("x%0Aghp_{FAKE_BODY}"),
        ),
        (
            "url-encoded-return-before-ghp",
            format!("%0Dghp_{FAKE_BODY}"),
        ),
        ("sha-before-ghp", format!("{sha}ghp_{FAKE_BODY}")),
        ("letter-before-gho", format!("xgho_{FAKE_BODY}")),
        ("letter-before-ghs", format!("xghs_{FAKE_BODY}")),
        ("letter-before-ghu", format!("xghu_{FAKE_BODY}")),
        ("letter-before-ghr", format!("xghr_{FAKE_BODY}")),
        (
            "letter-before-fine-grained",
            format!("xgithub_pat_{FAKE_BODY}"),
        ),
        ("letter-before-gitlab", format!("xglpat-{FAKE_BODY}")),
        (
            "escaped-newline-before-gitlab",
            format!("a\\nglpat-{FAKE_BODY}"),
        ),
        ("letter-before-gitlab-deploy", format!("xgldt-{FAKE_BODY}")),
        ("letter-before-gitlab-runner", format!("xglrt-{FAKE_BODY}")),
        (
            "letter-before-gitlab-trigger",
            format!("xglptt-{FAKE_BODY}"),
        ),
        (
            "letter-before-gitlab-ci-build",
            format!("xglcbt-{FAKE_BODY}"),
        ),
        ("letter-before-gitlab-feed", format!("xglft-{FAKE_BODY}")),
        ("letter-before-gitlab-oauth", format!("xgloas-{FAKE_BODY}")),
        (
            "letter-before-gitlab-agent",
            format!("xglagent-{FAKE_BODY}"),
        ),
        ("letter-before-gitlab-scim", format!("xglsoat-{FAKE_BODY}")),
    ];
    let mut owned = Vec::new();
    for (label, form) in &forms {
        owned.push((
            format!("config-{label}"),
            CONFIG.to_string(),
            format!("[core]\n\tbare = true\n# {form}\n"),
        ));
    }
    sources_save_stops(&as_cases(&owned));
}

#[test]
fn a_token_prefix_without_a_token_shaped_body_is_saved() {
    sources_save_goes_on(&[
        (
            "prefix-in-prose",
            CONFIG,
            "[core]\n\tbare = true\n# a ghp_ token or a glpat- token is never written here\n",
        ),
        (
            "prefix-then-short-word",
            CONFIG,
            "[core]\n\tbare = true\n# see ghp_docs and github_pat_help for the glpat-guide\n",
        ),
        (
            "config-comment-prose-prefix",
            CONFIG,
            "[core]\n\tbare = true\n# ghp_ prefix explained\n",
        ),
    ]);
}
