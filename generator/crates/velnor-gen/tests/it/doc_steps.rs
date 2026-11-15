// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{RustMember, RustRoot, SkipReason, StepNeed, Target, TargetKind, TargetName};
use velnor_rust::{optional_steps, root_sources};

use crate::select::{facts, member, name, path};

fn library(doctest: bool) -> RustMember {
    target_of("a", TargetKind::Lib, doctest, "a/src/lib.rs")
}

fn target_of(name: &str, kind: TargetKind, doctest: bool, source: &str) -> RustMember {
    RustMember {
        targets: vec![Target {
            name: TargetName::new(name).unwrap_or_else(|e| unreachable!("{e}")),
            kinds: vec![kind],
            tests: true,
            doctest,
            source: Some(path(source)),
            requires_features: false,
        }],
        ..member(name, "a", &[])
    }
}

fn binary() -> RustMember {
    target_of("b", TargetKind::Bin, false, "a/src/lib.rs")
}

fn root(members: Vec<RustMember>) -> RustRoot {
    RustRoot {
        manifest: path("Cargo.toml"),
        members,
        depends_on: Vec::new(),
        external_dependencies: Vec::new(),
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
    DependsOutside,
    BinaryDependsOutside,
    Macros,
    TextTarget,
}

const TEXT_TARGET: &str = "a/src/lib.txt";

const FENCED: &str = "/// Adds.\n///\n/// ```\n/// assert!(true);\n/// ```\npub fn f() {}\n";

const CASES: [(&str, Kind, &str, bool, StepNeed, StepNeed); 39] = [
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
    (
        "a macro from a dependency outside the root",
        Kind::DependsOutside,
        "#[derive(Builder)]\nstruct S;\n",
        false,
        RUN,
        RUN,
    ),
    (
        "a dependency outside the root without a doctest library",
        Kind::BinaryDependsOutside,
        "",
        false,
        NO_LIBRARY,
        RUN,
    ),
    (
        "inner block doc after code on its line",
        Kind::Doctested,
        "pub mod m { /*! x */ }\n",
        false,
        RUN,
        RUN,
    ),
    (
        "outer block doc after code on its line",
        Kind::Doctested,
        "pub mod m { /** x */ fn f() {} }\n",
        false,
        RUN,
        RUN,
    ),
    (
        "line doc after code on its line",
        Kind::Doctested,
        "pub mod m { /// Adds one.\n fn f() {} }\n",
        false,
        NO_FENCE,
        RUN,
    ),
    (
        "doc attribute whose equals sign starts the next line",
        Kind::Doctested,
        "#[doc\n= \"x\"]\nfn f() {}\n",
        false,
        RUN,
        RUN,
    ),
    (
        "doc attribute spread over five lines",
        Kind::Doctested,
        "#[\n    doc\n    =\n    \"x\"\n]\nfn f() {}\n",
        false,
        RUN,
        RUN,
    ),
    (
        "doc argument of a macro",
        Kind::Doctested,
        "m!(doc = \"x\");\n",
        false,
        RUN,
        RUN,
    ),
    (
        "a target whose path has no rs extension",
        Kind::TextTarget,
        "pub mod m { /*! x */ }\n",
        false,
        RUN,
        RUN,
    ),
    (
        "docs written into a string by a proc-macro crate",
        Kind::Macros,
        "fn f() -> &'static str {\n    \"/// Generated\\npub fn g() {}\"\n}\n",
        false,
        RUN,
        RUN,
    ),
    (
        "a proc-macro crate that never mentions docs",
        Kind::Macros,
        "fn f() {}\n",
        false,
        NO_FENCE,
        NO_DOCS,
    ),
    (
        "a doc marker in a string of a crate without macros",
        Kind::Doctested,
        "const A: &str = \"/// x\";\n",
        false,
        NO_FENCE,
        NO_DOCS,
    ),
    (
        "a module file named by a path attribute",
        Kind::Doctested,
        "#[path = \"gen.txt\"]\nmod g;\n",
        false,
        RUN,
        RUN,
    ),
    (
        "include with a space before the bang",
        Kind::Doctested,
        "include !(\"gen.rs\");\n",
        false,
        RUN,
        RUN,
    ),
    (
        "literals that could hide a doc comment",
        Kind::Doctested,
        "const A: &str = \"/*\";\nconst B: &str = r#\"a \" b\"#;\nconst Q: char = '\"';\nfn f<'a>(x: &'a str) {}\n/// Adds.\nfn g() {}\n",
        false,
        NO_FENCE,
        RUN,
    ),
    (
        "a variable named path and a cfg naming a docs-only flag",
        Kind::Doctested,
        "fn f() {\n    let path = 1;\n    let doc = 2;\n}\n#[cfg(docsrs)]\nfn g() {}\n",
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
    let mut wrong = Vec::new();
    for (case, kind, source, full, doctests, docs) in CASES {
        let (member, file) = match kind {
            Kind::Doctested | Kind::DependsOutside => (library(true), "a/src/lib.rs"),
            Kind::NoDoctest => (library(false), "a/src/lib.rs"),
            Kind::Binary | Kind::BinaryDependsOutside => (binary(), "a/src/lib.rs"),
            Kind::Macros => (
                target_of("a", TargetKind::ProcMacro, true, "a/src/lib.rs"),
                "a/src/lib.rs",
            ),
            Kind::TextTarget => (
                target_of("a", TargetKind::Lib, true, TEXT_TARGET),
                TEXT_TARGET,
            ),
        };
        let sources = BTreeMap::from([(path(file), source.as_bytes().to_vec())]);
        let mut root = root(vec![member]);
        if matches!(kind, Kind::DependsOutside | Kind::BinaryDependsOutside) {
            root.external_dependencies = vec![name("serde_derive")];
        }
        let steps = optional_steps(&root, &sources, full);
        if (steps.doctests, steps.docs) != (doctests, docs) {
            wrong.push(format!(
                "{case}: got {:?}, want {:?}",
                (steps.doctests, steps.docs),
                (doctests, docs)
            ));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

const TRACKED: [&str; 22] = [
    "app/src/main.rs",
    "app/README.md",
    "vendor/ext/src/lib.rs",
    "shared/gen.rs",
    "macros/x.rs",
    "other/only.rs",
    "tools/tool/src/main.rs",
    "tools/tool/src/lib.txt",
    "tools/tool/src/notes.txt",
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
];

#[test]
fn a_root_reads_its_own_sources_its_members_and_what_its_packages_read_but_not_a_nested_root() {
    let mut facts = facts(vec![
        ("Cargo.toml", vec![member("app", "app", &[])]),
        (
            "tools/Cargo.toml",
            vec![
                RustMember {
                    targets: target_of("tool", TargetKind::Lib, true, "tools/tool/src/lib.txt")
                        .targets,
                    ..member("tool", "tools/tool", &[])
                },
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
    let tracked: BTreeSet<_> = TRACKED.into_iter().map(path).collect();
    let sources = |index: usize| -> Vec<String> {
        root_sources(&facts, &facts.rust_roots[index], &tracked)
            .unwrap_or_default()
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
            "tools/tool/src/lib.txt",
            "tools/tool/src/main.rs",
            "vendor/ext/Cargo.toml",
            "vendor/ext/src/lib.rs",
        ]
    );
}

#[test]
fn a_package_that_reads_the_whole_repository_reads_no_file_and_runs_both_steps() {
    let mut facts = facts(vec![(
        "Cargo.toml",
        vec![library(true), member("other", "other", &[])],
    )]);
    facts.compile_inputs = BTreeMap::from([(None, BTreeSet::from([name("a")]))]);
    let tracked: BTreeSet<_> = ["a/src/lib.rs", "logo.png"].into_iter().map(path).collect();
    let root = &facts.rust_roots[0];
    assert_eq!(root_sources(&facts, root, &tracked), None);
    let steps = optional_steps(root, &BTreeMap::new(), true);
    assert_eq!((steps.doctests, steps.docs), (RUN, RUN));
    let binary_root = RustRoot {
        members: vec![binary()],
        ..root.clone()
    };
    let steps = optional_steps(&binary_root, &BTreeMap::new(), true);
    assert_eq!((steps.doctests, steps.docs), (NO_LIBRARY, RUN));
}

#[test]
fn a_target_source_the_checkout_cannot_show_reads_no_file_and_runs_both_steps() {
    let tracked: BTreeSet<_> = ["a/src/lib.rs", "a/Cargo.toml", "Cargo.toml"]
        .into_iter()
        .map(path)
        .collect();
    for source in [None, Some(path("generated/lib.rs"))] {
        let mut member = library(true);
        member.targets[0].source = source;
        let facts = facts(vec![("Cargo.toml", vec![member])]);
        let root = &facts.rust_roots[0];
        assert_eq!(root_sources(&facts, root, &tracked), None);
    }
}
