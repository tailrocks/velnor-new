// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{RustMember, RustRoot, Target, TargetKind, TargetName};
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

mod added;
mod cases;

use self::added::ADDED;
use self::cases::{Kind, NO_LIBRARY, ORIGINAL, RUN, TEXT_TARGET};

#[test]
fn doctests_and_docs_are_left_out_only_when_no_source_could_produce_them() {
    let mut wrong = Vec::new();
    for &(case, kind, source, full, doctests, docs) in ORIGINAL.iter().chain(ADDED) {
        let (member, file) = match kind {
            Kind::Doctested | Kind::DependsOutside | Kind::WithBuildScript => {
                (library(true), "a/src/lib.rs")
            }
            Kind::NoDoctest => (library(false), "a/src/lib.rs"),
            Kind::ConfigFile => (library(true), "a/.cargo/config.toml"),
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
        if matches!(kind, Kind::WithBuildScript) {
            root.members[0]
                .targets
                .extend(target_of("build", TargetKind::BuildScript, false, "a/build.rs").targets);
        }
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

#[test]
fn a_doctest_is_known_only_when_a_source_has_a_fenced_example() {
    let fenced = "/// ```\n/// assert!(true);\n/// ```\npub fn f() {}\n";
    let plain = "/// Adds one.\npub fn f() {}\n";
    for (source, dependencies, known) in [
        (plain, false, false),
        (plain, true, false),
        (fenced, false, true),
        (fenced, true, true),
    ] {
        let mut root = root(vec![library(true)]);
        if dependencies {
            root.external_dependencies = vec![name("serde_derive")];
        }
        let sources = BTreeMap::from([(path("a/src/lib.rs"), source.as_bytes().to_vec())]);
        let steps = optional_steps(&root, &sources, false);
        assert_eq!(steps.doctests_known, known, "{source} {dependencies}");
        assert_eq!(steps.doctests == RUN, known || dependencies);
    }
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

#[test]
fn a_missing_source_of_a_target_the_documentation_commands_never_select_is_not_a_doubt() {
    let tracked: BTreeSet<_> = ["a/src/lib.rs", "a/Cargo.toml", "Cargo.toml"]
        .into_iter()
        .map(path)
        .collect();
    for kind in [TargetKind::Test, TargetKind::Bench, TargetKind::Example] {
        let mut member = library(true);
        member
            .targets
            .extend(target_of("t", kind, false, "generated/t.rs").targets);
        let facts = facts(vec![("Cargo.toml", vec![member])]);
        let root = &facts.rust_roots[0];
        assert!(root_sources(&facts, root, &tracked).is_some());
    }
}
