// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_ci::plan::Steps;
use velnor_ci::plan::{explain, outputs};
use velnor_model::{
    AllTrigger, OptionalSteps, PackageName, RepoPath, RustSelection, SelectionReason, SkipReason,
    StepNeed,
};

fn path(text: &str) -> RepoPath {
    RepoPath::new(text).unwrap_or_else(|e| unreachable!("{e}"))
}

fn name(text: &str) -> PackageName {
    PackageName::new(text).unwrap_or_else(|e| unreachable!("{e}"))
}

fn steps(doctests: StepNeed, docs: StepNeed) -> OptionalSteps {
    OptionalSteps { doctests, docs }
}

const SKIP_DOCS: StepNeed = StepNeed::Skip(SkipReason::NoDocs);
const NO_LIBRARY: StepNeed = StepNeed::Skip(SkipReason::NoDoctestLibrary);

fn three_roots() -> Steps {
    BTreeMap::from([
        (path("Cargo.toml"), steps(StepNeed::Run, StepNeed::Run)),
        (path("ci-helper/Cargo.toml"), steps(NO_LIBRARY, SKIP_DOCS)),
        (
            path("crates/velnor-runner/Cargo.toml"),
            steps(NO_LIBRARY, SKIP_DOCS),
        ),
    ])
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
        outputs(&selections, &three_roots()).unwrap_or_else(|e| unreachable!("{e}")),
        "rust_root_mode=all\n\
         rust_root_packages=\n\
         rust_root_test_filter=all()\n\
         rust_root_doctests=run\n\
         rust_root_docs=run\n\
         rust_ci_helper_mode=some\n\
         rust_ci_helper_packages=support velnor-ci\n\
         rust_ci_helper_test_filter=package(support) | package(velnor-ci)\n\
         rust_ci_helper_doctests=skip\n\
         rust_ci_helper_docs=skip\n\
         rust_crates_velnor_runner_mode=none\n\
         rust_crates_velnor_runner_packages=\n\
         rust_crates_velnor_runner_test_filter=\n\
         rust_crates_velnor_runner_doctests=skip\n\
         rust_crates_velnor_runner_docs=skip\n"
    );
    let text = explain(
        &selections,
        &three_roots(),
        &BTreeSet::new(),
        &BTreeMap::new(),
    );
    assert_eq!(
        text,
        "Cargo.toml: all:unknown-path:docs/goals.md\n\
         ci-helper/Cargo.toml: some\n  \
         support depends-on:velnor-ci\n  \
         velnor-ci changed:ci-helper/crates/velnor-ci/src/plan.rs\n  \
         left out: doctests: no library target runs doctests\n  \
         left out: documentation: no doc comment or doc attribute exists\n\
         crates/velnor-runner/Cargo.toml: none\n"
    );
}

#[test]
fn two_roots_whose_output_names_would_collide_are_an_error() {
    let selections = BTreeMap::from([
        (path("a-b/Cargo.toml"), RustSelection::None),
        (path("a_b/Cargo.toml"), RustSelection::None),
    ]);
    assert!(outputs(&selections, &Steps::new()).is_err());
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
    let text = explain(&selections, &Steps::new(), &unknown, &BTreeMap::new());
    assert_eq!(text.matches("unknown path: scripts/gen.sh").count(), 1);
    assert!(text.contains("\n    \"scripts/gen.sh\",\n"), "{text}");
    assert!(
        text.contains("[[inputs]]\n    glob = \"scripts/gen.sh\"\n"),
        "{text}"
    );
    assert!(text.contains("\n    \"docs/a\\\"b.txt\",\n"), "{text}");

    assert!(
        !explain(
            &selections,
            &Steps::new(),
            &BTreeSet::new(),
            &BTreeMap::new()
        )
        .contains("unknown path")
    );

    let starred = explain(
        &selections,
        &Steps::new(),
        &BTreeSet::from([path("scripts/a*b")]),
        &BTreeMap::new(),
    );
    assert!(starred.contains("unknown path: scripts/a*b\n"), "{starred}");
    assert!(!starred.contains("glob = "), "{starred}");
    assert!(!starred.contains("\"scripts/a*b\""), "{starred}");
}
