// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_gen::GenerateError;
use velnor_model::{
    AllTrigger, ChangeSet, InputOwner, PackageName, RepoView, RustSelection, SelectionReason,
};
use velnor_rust::{
    DetectError, INPUTS_LOCK, Owners, Reason, add_reads, detect, format_inputs_lock,
    parse_inputs_lock, select, without_identity,
};

use crate::facts::{TestResult, lib, repo};
use crate::pipeline::{HELPER_LOCK, view, workflow};
use crate::select::{name, path};

const A: &str = "a/util/Cargo.toml";
const B: &str = "b/util/Cargo.toml";

fn two_utils(lock: Option<&str>) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut files = vec![
        (
            "a/util/src/lib.rs",
            "const A: &str = include_str!(\"../../../docs/a.md\");",
        ),
        (
            "b/util/src/lib.rs",
            "const B: &str = include_str!(\"../../../docs/b.md\");",
        ),
    ];
    if let Some(lock) = lock {
        files.push((".velnor/rust-inputs.lock", lock));
    }
    repo(
        vec![
            ("a/Cargo.toml", vec![lib("util", A)?]),
            ("b/Cargo.toml", vec![lib("util", B)?]),
        ],
        &files,
    )
}

fn selection(file: &str, facts: &velnor_model::RepoFacts) -> BTreeMap<String, RustSelection> {
    let changes = ChangeSet {
        paths: BTreeSet::from([path(file)]),
        base_files: BTreeMap::new(),
        head_files: BTreeMap::new(),
    };
    select(facts, &changes)
        .into_iter()
        .map(|(root, selection)| (root.to_string(), selection))
        .collect()
}

/// A root whose only package is `util` selects "most" packages when that package is selected.
fn selected(file: &str) -> RustSelection {
    RustSelection::All(AllTrigger::MostPackages(BTreeMap::from([(
        name("util"),
        SelectionReason::Input(path(file)),
    )])))
}

#[test]
fn two_packages_of_one_name_in_different_roots_keep_their_own_embedded_inputs() -> TestResult {
    let facts = detect(&two_utils(None)?)?;
    let owner = |file: &str| -> Vec<InputOwner> {
        facts
            .compile_inputs
            .get(&Some(path(file)))
            .into_iter()
            .flatten()
            .cloned()
            .collect()
    };
    assert_eq!(owner("docs/a.md"), [InputOwner::at(name("util"), path(A))]);
    assert_eq!(owner("docs/b.md"), [InputOwner::at(name("util"), path(B))]);
    assert_eq!(
        selection("docs/a.md", &facts),
        BTreeMap::from([
            ("a/Cargo.toml".to_owned(), selected("docs/a.md")),
            ("b/Cargo.toml".to_owned(), RustSelection::None),
        ])
    );
    assert_eq!(
        selection("docs/b.md", &facts),
        BTreeMap::from([
            ("a/Cargo.toml".to_owned(), RustSelection::None),
            ("b/Cargo.toml".to_owned(), selected("docs/b.md")),
        ])
    );
    Ok(())
}

#[test]
fn a_lock_line_with_a_manifest_selects_only_that_package_and_a_name_only_line_selects_every_one()
-> TestResult {
    let identified = detect(&two_utils(Some(&format!("data/x.txt\tutil\t{B}\n")))?)?;
    assert!(identified.inputs_lock_identified);
    assert_eq!(
        selection("data/x.txt", &identified),
        BTreeMap::from([
            ("a/Cargo.toml".to_owned(), RustSelection::None),
            ("b/Cargo.toml".to_owned(), selected("data/x.txt")),
        ])
    );
    let legacy = detect(&two_utils(Some("data/x.txt\tutil\n"))?)?;
    assert!(!legacy.inputs_lock_identified);
    assert_eq!(
        selection("data/x.txt", &legacy),
        BTreeMap::from([
            ("a/Cargo.toml".to_owned(), selected("data/x.txt")),
            ("b/Cargo.toml".to_owned(), selected("data/x.txt")),
        ])
    );
    Ok(())
}

#[test]
fn a_lock_line_naming_a_manifest_that_is_not_that_package_is_an_error() -> TestResult {
    for lock in [
        "data/x.txt\tutil\tc/util/Cargo.toml\n",
        "data/x.txt\tother\ta/util/Cargo.toml\n",
    ] {
        let Err(DetectError::Invalid { reason, .. }) = detect(&two_utils(Some(lock))?) else {
            return Err(format!("{lock:?} was accepted").into());
        };
        assert!(
            matches!(reason, Reason::UnknownLockedPackage(_)),
            "{lock:?}: {reason:?}"
        );
    }
    let Err(DetectError::Invalid { reason, .. }) =
        detect(&two_utils(Some("data/x.txt\tutil\ta\tb\n"))?)
    else {
        return Err("a fourth column was accepted".into());
    };
    assert_eq!(reason, Reason::LockLine(1));
    Ok(())
}

#[test]
fn a_lock_written_before_manifests_still_parses_and_a_lock_with_them_round_trips() -> TestResult {
    let old = "README.md\tapp\nshared/a.txt\tapp\n.\tlib\n";
    let parsed = parse_inputs_lock(old.as_bytes())?;
    assert_eq!(
        parsed.get(&Some(path("README.md"))),
        Some(&BTreeSet::from([InputOwner::named(name("app"))]))
    );
    assert_eq!(
        format_inputs_lock(&parsed),
        ".\tlib\nREADME.md\tapp\nshared/a.txt\tapp\n"
    );

    let new = "a.txt\tutil\ta/util/Cargo.toml\na.txt\tutil\tb/util/Cargo.toml\nb.txt\tapp\n";
    let parsed = parse_inputs_lock(new.as_bytes())?;
    assert_eq!(format_inputs_lock(&parsed), new);
    assert_eq!(
        format_inputs_lock(&without_identity(&parsed)),
        "a.txt\tutil\nb.txt\tapp\n"
    );
    // A velnor-ci before 0.1.100 splits at the first tab only, so the manifest becomes part of
    // the package name, which a name may not hold: it refuses the line instead of misreading it.
    assert!(PackageName::new("util\ta/util/Cargo.toml").is_err());
    Ok(())
}

#[test]
fn each_package_of_a_shared_name_is_attributed_its_own_reads() -> TestResult {
    let facts = detect(&two_utils(None)?)?;
    let tracked: BTreeSet<_> = [
        "a/util/src/lib.rs",
        "b/util/src/lib.rs",
        "docs/a.md",
        "docs/b.md",
    ]
    .into_iter()
    .map(path)
    .collect();
    let owners = Owners::new(&facts, &tracked);
    let mut found = BTreeMap::new();
    for (source, read) in [
        ("a/util/src/lib.rs", "docs/a.md"),
        ("b/util/src/lib.rs", "docs/b.md"),
    ] {
        let source = path(source);
        for owner in owners.of(Some("util"), &source) {
            add_reads(&mut found, &tracked, &owner, &[Some(path(read))], true, &[]);
        }
    }
    assert_eq!(
        format_inputs_lock(&found),
        format!("docs/a.md\tutil\t{A}\ndocs/b.md\tutil\t{B}\n")
    );
    Ok(())
}

fn pinned(version: &str, lock: &str) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut files = view(&["Cargo.toml"], None)?;
    files.files.insert(
        velnor_rust::LOCK_PATH,
        String::from_utf8_lossy(HELPER_LOCK)
            .replace("0.1.81", version)
            .into_bytes(),
    );
    files.files.insert(INPUTS_LOCK, lock.as_bytes().to_vec());
    Ok(files)
}

#[test]
fn a_lock_with_manifests_needs_a_helper_that_reads_them() -> TestResult {
    let identified = "README.md\tpackage-0\tCargo.toml\n";
    let error = velnor_gen::expected_with_notes(&pinned("0.1.99", identified)?)
        .expect_err("the 0.1.99 pin was accepted");
    let GenerateError::InputsLockHelperTooOld(version) = &error else {
        return Err(format!("unexpected error: {error}").into());
    };
    assert_eq!(version.to_string(), "0.1.99");
    assert!(error.to_string().contains("0.1.100"), "{error}");

    workflow(&pinned("0.1.100", identified)?)?;
    workflow(&pinned("0.1.99", "README.md\tpackage-0\n")?)?;
    Ok(())
}
