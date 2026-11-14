// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_ci::plan::{explain, outputs};
use velnor_model::{AllTrigger, PackageName, RepoPath, RustSelection, SelectionReason};

fn path(text: &str) -> RepoPath {
    RepoPath::new(text).unwrap_or_else(|e| unreachable!("{e}"))
}

fn name(text: &str) -> PackageName {
    PackageName::new(text).unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn each_root_gets_its_mode_its_packages_and_a_nextest_filter_never_cargo_arguments() {
    let selected = BTreeMap::from([
        (
            name("velnor-ci"),
            SelectionReason::Changed(path("ci-helper/crates/velnor-ci/src/plan.rs")),
        ),
        (
            name("support"),
            SelectionReason::DependsOn(name("velnor-ci")),
        ),
    ]);
    let selections = BTreeMap::from([
        (
            path("Cargo.toml"),
            RustSelection::All(AllTrigger::UnknownPath(path("docs/goals.md"))),
        ),
        (path("ci-helper/Cargo.toml"), RustSelection::Some(selected)),
        (path("crates/velnor-runner/Cargo.toml"), RustSelection::None),
    ]);
    assert_eq!(
        outputs(&selections).unwrap_or_else(|e| unreachable!("{e}")),
        "rust_root_mode=all\n\
         rust_root_packages=\n\
         rust_root_test_filter=all()\n\
         rust_ci_helper_mode=some\n\
         rust_ci_helper_packages=support velnor-ci\n\
         rust_ci_helper_test_filter=package(support) | package(velnor-ci)\n\
         rust_crates_velnor_runner_mode=none\n\
         rust_crates_velnor_runner_packages=\n\
         rust_crates_velnor_runner_test_filter=\n"
    );
}

#[test]
fn two_roots_whose_output_names_would_collide_are_an_error() {
    let selections = BTreeMap::from([
        (path("a-b/Cargo.toml"), RustSelection::None),
        (path("a_b/Cargo.toml"), RustSelection::None),
    ]);
    assert!(outputs(&selections).is_err());
}

#[test]
fn every_unknown_path_is_explained_once_with_lines_that_would_classify_it() {
    let selections = BTreeMap::from([
        (
            path("Cargo.toml"),
            RustSelection::All(AllTrigger::CargoConfig(path(".cargo/config.toml"))),
        ),
        (
            path("tools/Cargo.toml"),
            RustSelection::All(AllTrigger::CargoConfig(path(".cargo/config.toml"))),
        ),
    ]);
    let unknown = BTreeSet::from([path("scripts/gen.sh"), path("docs/a\"b.txt")]);
    let text = explain(&selections, &unknown);
    assert_eq!(text.matches("unknown path: scripts/gen.sh").count(), 1);
    assert!(text.contains("\n    \"scripts/gen.sh\",\n"), "{text}");
    assert!(
        text.contains("[[inputs]]\n    glob = \"scripts/gen.sh\"\n"),
        "{text}"
    );
    assert!(text.contains("\n    \"docs/a\\\"b.txt\",\n"), "{text}");

    assert!(!explain(&selections, &BTreeSet::new()).contains("unknown path"));
}
