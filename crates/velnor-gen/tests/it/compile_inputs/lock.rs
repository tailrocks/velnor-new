// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_model::{InputKey, InputOwner, RepoFacts, RustSelection, SelectionReason};
use velnor_rust::{DetectError, INPUTS_LOCK, Reason, detect};

use crate::facts::{TestResult, lib, repo};
use crate::select::{assert_selects, changed, glob, name, named, path, some, two_roots, unknown};

fn locked(facts: &mut RepoFacts, entries: &[(&str, &str)]) {
    for (file, package) in entries {
        facts
            .compile_inputs
            .entry(InputKey::Path(path(file)))
            .or_default()
            .insert(InputOwner::named(name(package)));
    }
}

fn input(file: &str) -> SelectionReason {
    SelectionReason::Input(path(file))
}

#[test]
fn a_path_the_inputs_lock_lists_selects_its_packages_even_when_rules_ignore_it() {
    let none = RustSelection::None;
    let mut facts = two_roots();
    assert_selects(
        &facts,
        vec![(
            &["shared-data/greeting.txt"],
            vec![
                unknown("shared-data/greeting.txt"),
                unknown("shared-data/greeting.txt"),
            ],
        )],
    );
    locked(
        &mut facts,
        &[
            ("shared-data/greeting.txt", "app"),
            ("README.md", "app"),
            ("docs/schema", "cli"),
            ("app/src/schema.json", "tool"),
        ],
    );
    facts.config.non_inputs = vec![glob("docs/**")];
    assert_selects(
        &facts,
        vec![
            (
                &["shared-data/greeting.txt"],
                vec![
                    some(&[("app", input("shared-data/greeting.txt"))]),
                    none.clone(),
                ],
            ),
            (
                &["README.md"],
                vec![some(&[("app", input("README.md"))]), none.clone()],
            ),
            (
                &["docs/schema/v1/a.json"],
                vec![
                    none.clone(),
                    some(&[("cli", input("docs/schema/v1/a.json"))]),
                ],
            ),
            (&["docs/schema.md"], vec![none.clone(), none.clone()]),
            (
                &["app/src/schema.json"],
                vec![
                    some(&[
                        ("app", changed("app/src/schema.json")),
                        ("tool", input("app/src/schema.json")),
                    ]),
                    none.clone(),
                ],
            ),
        ],
    );

    let mut everywhere = two_roots();
    everywhere.compile_inputs.insert(
        InputKey::Everything,
        BTreeSet::from([InputOwner::named(name("cli"))]),
    );
    assert_selects(
        &everywhere,
        vec![
            (
                &["CHANGELOG.md"],
                vec![none.clone(), some(&[("cli", input("CHANGELOG.md"))])],
            ),
            (
                &["unknown/file.bin"],
                vec![unknown("unknown/file.bin"), unknown("unknown/file.bin")],
            ),
        ],
    );
}

#[test]
fn the_inputs_lock_becomes_facts_and_a_bad_line_or_an_unknown_package_is_an_error() -> TestResult {
    let view = |lock: &str| {
        repo(
            vec![("Cargo.toml", vec![lib("app", "app/Cargo.toml")?])],
            &[(INPUTS_LOCK.as_str(), lock)],
        )
    };
    assert_eq!(
        detect(&view(".\tapp\nREADME.md\tapp\nshared/a.txt\tapp\n")?)?.compile_inputs,
        BTreeMap::from([
            (InputKey::Everything, BTreeSet::from([named("app")])),
            (
                InputKey::Path(path("README.md")),
                BTreeSet::from([named("app")])
            ),
            (
                InputKey::Path(path("shared/a.txt")),
                BTreeSet::from([named("app")])
            ),
        ])
    );
    for (lock, reason) in [
        ("README.md\tapp\nshared/a.txt app\n", Reason::LockLine(2)),
        ("../a.txt\tapp\n", Reason::LockLine(1)),
        (
            "README.md\tapplication\n",
            Reason::UnknownLockedPackage("application".to_owned()),
        ),
    ] {
        assert_eq!(
            detect(&view(lock)?),
            Err(DetectError::Invalid {
                path: INPUTS_LOCK,
                reason,
            }),
            "{lock}"
        );
    }
    Ok(())
}

#[test]
fn a_dot_line_in_the_inputs_lock_reads_everything_and_is_not_an_unresolved_include() -> TestResult {
    let view = repo(
        vec![(
            "Cargo.toml",
            vec![lib("app", "app/Cargo.toml")?, lib("cli", "cli/Cargo.toml")?],
        )],
        &[
            (INPUTS_LOCK.as_str(), ".\tcli\n"),
            ("app/src/lib.rs", "include_str!(\"../../docs/guide.md\");"),
        ],
    )?;
    let facts = detect(&view)?;
    assert_eq!(
        facts.compile_inputs.get(&InputKey::Everything),
        Some(&BTreeSet::from([named("cli")]))
    );
    let changes = velnor_model::ChangeSet {
        paths: BTreeSet::from([path("CHANGELOG.md")]),
        ..velnor_model::ChangeSet::default()
    };
    let chosen: Vec<(String, String)> =
        match velnor_rust::select(&facts, &changes).into_values().next() {
            Some(RustSelection::Some(map)) => map
                .into_iter()
                .map(|(name, reason)| (name.to_string(), reason.to_string()))
                .collect(),
            other => panic!("{other:?}"),
        };
    assert_eq!(
        chosen,
        [("cli".to_owned(), "input:CHANGELOG.md".to_owned())]
    );
    Ok(())
}
