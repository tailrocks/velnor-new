// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{SkipReason, StepNeed};

pub(super) type Case = (&'static str, Kind, &'static str, bool, StepNeed, StepNeed);

pub(super) const RUN: StepNeed = StepNeed::Run;
pub(super) const NO_LIBRARY: StepNeed = StepNeed::Skip(SkipReason::NoDoctestLibrary);
pub(super) const NO_FENCE: StepNeed = StepNeed::Skip(SkipReason::NoFencedDocs);
pub(super) const NO_DOCS: StepNeed = StepNeed::Skip(SkipReason::NoDocs);

#[derive(Clone, Copy)]
pub(super) enum Kind {
    Doctested,
    NoDoctest,
    Binary,
    DependsOutside,
    BinaryDependsOutside,
    Macros,
    TextTarget,
    ConfigFile,
    WithBuildScript,
}

pub(super) const TEXT_TARGET: &str = "a/src/lib.txt";

pub(super) const FENCED: &str =
    "/// Adds.\n///\n/// ```\n/// assert!(true);\n/// ```\npub fn f() {}\n";

pub(super) const ORIGINAL: &[Case] = &[
    ("fenced line doc", Kind::Doctested, FENCED, false, RUN, RUN),
    (
        "fenced inner doc",
        Kind::Doctested,
        "//! ~~~\n//! x\n//! ~~~\n",
        false,
        RUN,
        RUN,
    ),
    (
        "fenced block doc",
        Kind::Doctested,
        "/**\n * ```\n * x\n * ```\n */\n",
        false,
        RUN,
        RUN,
    ),
    (
        "include_str doc",
        Kind::Doctested,
        "#![doc = include_str!(\"../README.md\")]\n",
        false,
        RUN,
        RUN,
    ),
    (
        "cfg_attr doc",
        Kind::Doctested,
        "#[cfg_attr(docsrs, doc = \"x\")]\n",
        false,
        RUN,
        RUN,
    ),
    (
        "unfenced doc",
        Kind::Doctested,
        "/// Adds one.\npub fn f() {}\n",
        false,
        NO_FENCE,
        RUN,
    ),
    (
        "doctest = false",
        Kind::NoDoctest,
        FENCED,
        false,
        NO_LIBRARY,
        RUN,
    ),
    ("binary only", Kind::Binary, FENCED, false, NO_LIBRARY, RUN),
    (
        "doc attribute without text",
        Kind::Doctested,
        "#![doc(html_root_url = \"x\")]\n",
        false,
        RUN,
        RUN,
    ),
    (
        "plain comments",
        Kind::Doctested,
        "// ```\n//// ```\n/*** x */\n/**/\nfn f() {}\n",
        false,
        NO_FENCE,
        NO_DOCS,
    ),
    (
        "a block doc may hold an example",
        Kind::Doctested,
        "/** ``` x ``` */\nfn f() {}\n",
        false,
        RUN,
        RUN,
    ),
    (
        "fence in a block quote",
        Kind::Doctested,
        "/// > ```\n/// > assert!(true);\n/// > ```\npub fn f() {}\n",
        false,
        RUN,
        RUN,
    ),
    (
        "code only rustdoc compiles",
        Kind::Doctested,
        "#[cfg(doc)]\npub struct OnlyInDocs;\n",
        false,
        RUN,
        RUN,
    ),
    (
        "a cargo config that includes another",
        Kind::ConfigFile,
        "include = \"ci.toml\"\n",
        false,
        NO_FENCE,
        RUN,
    ),
    (
        "a cfg naming a feature called docs",
        Kind::Doctested,
        "#[cfg(feature = \"docs\")]\nfn f() {}\n",
        false,
        NO_FENCE,
        NO_DOCS,
    ),
    (
        "tokens included from a generated file",
        Kind::Doctested,
        "include!(concat!(env!(\"OUT_DIR\"), \"/generated.rs\"));\n",
        false,
        RUN,
        RUN,
    ),
    (
        "indented code block",
        Kind::Doctested,
        "/// Adds.\n///\n///     assert!(true);\npub fn f() {}\n",
        false,
        RUN,
        RUN,
    ),
    (
        "assignment named doc",
        Kind::Doctested,
        "fn f() {\n    let doc = 1;\n}\n",
        false,
        RUN,
        RUN,
    ),
    (
        "multi-line cfg_attr doc",
        Kind::Doctested,
        "#[cfg_attr(\n    docsrs,\n    doc = \"x\"\n)]\nfn f() {}\n",
        false,
        RUN,
        RUN,
    ),
    (
        "rustdoc lint",
        Kind::Doctested,
        "#![deny(rustdoc::broken_intra_doc_links)]\n",
        false,
        NO_FENCE,
        RUN,
    ),
    (
        "missing_docs in a manifest",
        Kind::Doctested,
        "[lints.rust]\nmissing_docs = \"deny\"\n",
        false,
        NO_FENCE,
        RUN,
    ),
    ("scheduled full run", Kind::Doctested, "", true, RUN, RUN),
    (
        "scheduled run without a library",
        Kind::Binary,
        "",
        true,
        NO_LIBRARY,
        RUN,
    ),
];
