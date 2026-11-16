// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const FENCED: &str = "/// ```\n/// assert!(true);\n/// ```\npub fn f() {}\n";
const README: &str = "```\nlet x = 1;\n```\n";
const READS_README: &str = "#![doc = include_str!(\"../README.md\")]\nfn documented() {}\n";

type Files = &'static [(&'static str, &'static str)];

pub(super) const READ: &[(&str, Files, bool, bool)] = &[
    (
        "unread README",
        &[
            ("a/src/lib.rs", "pub fn documented() {}\n"),
            ("a/README.md", README),
        ],
        false,
        true,
    ),
    (
        "README read by doc include",
        &[("a/src/lib.rs", READS_README), ("a/README.md", README)],
        true,
        true,
    ),
    (
        "README read by doc include without a fence",
        &[
            ("a/src/lib.rs", READS_README),
            ("a/README.md", "Just words.\n"),
        ],
        false,
        true,
    ),
    (
        "doc include of a path no source shows",
        &[("a/src/lib.rs", READS_README)],
        false,
        true,
    ),
    (
        "doc include escaping the checkout",
        &[
            ("a/src/lib.rs", "#![doc = include_str!(\"/a/README.md\")]\n"),
            ("a/README.md", README),
        ],
        false,
        true,
    ),
    (
        "doc attribute literal with a fence",
        &[(
            "a/src/lib.rs",
            "#[doc = \"```\\nlet x = 1;\\n```\"]\npub fn f() {}\n",
        )],
        true,
        true,
    ),
    (
        "doc attribute literal without a fence",
        &[("a/src/lib.rs", "#[doc = \"Adds one.\"]\npub fn f() {}\n")],
        false,
        true,
    ),
    (
        "block comment without a fence",
        &[("a/src/lib.rs", "/** Adds one. */\npub fn f() {}\n")],
        false,
        false,
    ),
    (
        "block comment with a fence",
        &[(
            "a/src/lib.rs",
            "/**\n * ```\n * let x = 1;\n * ```\n */\npub fn f() {}\n",
        )],
        true,
        true,
    ),
    (
        "fence in the library",
        &[("a/src/lib.rs", FENCED)],
        true,
        true,
    ),
    (
        "fence only in a binary",
        &[
            ("a/src/lib.rs", "pub fn f() {}\n"),
            ("a/src/main.rs", FENCED),
        ],
        false,
        true,
    ),
    (
        "text fence in the library",
        &[(
            "a/src/lib.rs",
            "/// ```text\n/// x\n/// ```\npub fn f() {}\n",
        )],
        false,
        true,
    ),
    (
        "text fence closed then no test",
        &[(
            "a/src/lib.rs",
            "/**\n * ```python\n * x\n * ```\n */\npub fn f() {}\n",
        )],
        false,
        true,
    ),
    (
        "rustdoc attribute fence",
        &[(
            "a/src/lib.rs",
            "/// ```should_panic,edition2021\n/// x\n/// ```\npub fn f() {}\n",
        )],
        true,
        true,
    ),
    (
        "rust fence after a text fence",
        &[(
            "a/src/lib.rs",
            "/// ```text\n/// x\n/// ```\n/// ```rust\n/// y\n/// ```\npub fn f() {}\n",
        )],
        true,
        true,
    ),
    (
        "doc assignment in ordinary code",
        &[("a/src/lib.rs", "pub fn f() {\n    let doc = \"```\";\n}\n")],
        false,
        true,
    ),
    (
        "doc assignment after an attribute",
        &[(
            "a/src/lib.rs",
            "#[allow(dead_code)]\nfn f() {\n    let doc = \"```\";\n}\n",
        )],
        false,
        true,
    ),
    (
        "cfg_attr doc literal with a fence",
        &[(
            "a/src/lib.rs",
            "#[cfg_attr(all(), doc = \"```\\nx\\n```\")]\npub fn f() {}\n",
        )],
        true,
        true,
    ),
    (
        "inner doc attribute literal with a fence",
        &[("a/src/lib.rs", "#![doc = \"```\\nx\\n```\"]\n")],
        true,
        true,
    ),
    (
        "block doc whose summary starts on the opening line",
        &[(
            "a/src/lib.rs",
            "/** Adds one.\n *     assert!(true);\n */\npub fn f() {}\n",
        )],
        true,
        true,
    ),
    (
        "doc literal with an indented code block",
        &[(
            "a/src/lib.rs",
            "#[doc = r\"    assert!(true);\"]\npub fn f() {}\n",
        )],
        true,
        true,
    ),
    (
        "block doc and indented prose without code",
        &[(
            "a/src/lib.rs",
            "/** Adds one. */\n/**\n  Adds.\n  More.\n*/\npub fn f() {}\n",
        )],
        false,
        false,
    ),
];
