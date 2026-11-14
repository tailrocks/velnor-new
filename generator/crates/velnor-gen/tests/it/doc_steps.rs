// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{RustMember, RustRoot, SkipReason, StepNeed, Target, TargetKind, TargetName};
use velnor_rust::{optional_steps, root_sources};

use crate::select::{facts, member, name, path};

fn library(doctest: bool) -> RustMember {
    RustMember {
        targets: vec![Target {
            name: TargetName::new("a").unwrap_or_else(|e| unreachable!("{e}")),
            kinds: vec![TargetKind::Lib],
            tests: true,
            doctest,
            source: None,
            requires_features: false,
        }],
        ..member("a", "a", &[])
    }
}

fn binary() -> RustMember {
    RustMember {
        targets: vec![Target {
            name: TargetName::new("b").unwrap_or_else(|e| unreachable!("{e}")),
            kinds: vec![TargetKind::Bin],
            tests: true,
            doctest: false,
            source: None,
            requires_features: false,
        }],
        ..member("b", "b", &[])
    }
}

fn root(members: Vec<RustMember>) -> RustRoot {
    RustRoot {
        manifest: path("Cargo.toml"),
        members,
        depends_on: Vec::new(),
    }
}

const RUN: StepNeed = StepNeed::Run;
const NO_LIBRARY: StepNeed = StepNeed::Skip(SkipReason::NoDoctestLibrary);
const NO_FENCE: StepNeed = StepNeed::Skip(SkipReason::NoFencedDocs);
const NO_DOCS: StepNeed = StepNeed::Skip(SkipReason::NoDocs);

#[derive(Clone, Copy)]
enum Kind {
    Doctested,
    NoDoctest,
    Binary,
}

const FENCED: &str = "/// Adds.\n///\n/// ```\n/// assert!(true);\n/// ```\npub fn f() {}\n";

const CASES: [(&str, Kind, &str, bool, StepNeed, StepNeed); 22] = [
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
        NO_FENCE,
        NO_DOCS,
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
        "/** x */\nfn f() {}\n",
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
        NO_FENCE,
        RUN,
    ),
    (
        "a cargo config that includes another",
        Kind::Doctested,
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
        NO_FENCE,
        NO_DOCS,
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

#[test]
fn doctests_and_docs_are_left_out_only_when_no_source_could_produce_them() {
    for (case, kind, source, full, doctests, docs) in CASES {
        let member = match kind {
            Kind::Doctested => library(true),
            Kind::NoDoctest => library(false),
            Kind::Binary => binary(),
        };
        let sources = BTreeMap::from([(path("a/src/lib.rs"), source.as_bytes().to_vec())]);
        let steps = optional_steps(&root(vec![member]), &sources, full);
        assert_eq!((steps.doctests, steps.docs), (doctests, docs), "{case}");
    }
}

#[test]
fn a_root_reads_its_own_sources_its_members_and_what_its_packages_read_but_not_a_nested_root() {
    let mut facts = facts(vec![
        ("Cargo.toml", vec![member("app", "app", &[])]),
        (
            "tools/Cargo.toml",
            vec![
                member("tool", "tools/tool", &[]),
                member("ext", "vendor/ext", &[]),
            ],
        ),
    ]);
    facts.compile_inputs = BTreeMap::from([
        (Some(path("shared/gen.rs")), BTreeSet::from([name("tool")])),
        (Some(path("shared/gen.inc")), BTreeSet::from([name("tool")])),
        (Some(path("macros")), BTreeSet::from([name("tool")])),
        (Some(path("other/only.rs")), BTreeSet::from([name("app")])),
    ]);
    let tracked: BTreeSet<_> = [
        "app/src/main.rs",
        "app/README.md",
        "vendor/ext/src/lib.rs",
        "shared/gen.rs",
        "macros/x.rs",
        "other/only.rs",
        "tools/tool/src/main.rs",
        "build.RS",
        "shared/gen.inc",
        "macros/template.txt",
        "app/notes.txt",
        "Cargo.toml",
        "app/Cargo.toml",
        "tools/Cargo.toml",
        "tools/tool/Cargo.toml",
        "vendor/ext/Cargo.toml",
        "other/Cargo.toml",
        ".cargo/config.toml",
        "tools/.cargo/config.toml",
        "other/.cargo/config.toml",
    ]
    .into_iter()
    .map(path)
    .collect();
    let sources = |index: usize| -> Vec<String> {
        root_sources(&facts, &facts.rust_roots[index], &tracked)
            .iter()
            .map(ToString::to_string)
            .collect()
    };
    assert_eq!(
        sources(0),
        [
            ".cargo/config.toml",
            "Cargo.toml",
            "app/Cargo.toml",
            "app/src/main.rs",
            "build.RS",
            "macros/x.rs",
            "other/only.rs",
            "shared/gen.rs",
            "vendor/ext/src/lib.rs",
        ]
    );
    assert_eq!(
        sources(1),
        [
            ".cargo/config.toml",
            "macros/template.txt",
            "macros/x.rs",
            "shared/gen.inc",
            "shared/gen.rs",
            "tools/.cargo/config.toml",
            "tools/Cargo.toml",
            "tools/tool/Cargo.toml",
            "tools/tool/src/main.rs",
            "vendor/ext/Cargo.toml",
            "vendor/ext/src/lib.rs",
        ]
    );
}
