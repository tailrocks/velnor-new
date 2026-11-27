// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoView;

use super::world::{QUIET, put, runs_every, skips_every, world};
use crate::facts::TestResult;

const EXTERNALS: &str = ".velnor/generator-externals.lock";
const CRATES_IO: &str = "registry+https://github.com/rust-lang/crates.io-index";
const REGISTRY: &str =
    "source = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"sum\"\n";

fn package(name: &str, version: &str, source: bool, dependencies: &[&str]) -> String {
    let listed: Vec<String> = dependencies.iter().map(|d| format!("\"{d}\"")).collect();
    let source = if source { REGISTRY } else { "" };
    format!(
        "[[package]]\nname = \"{name}\"\nversion = \"{version}\"\n{source}dependencies = [{}]\n\n",
        listed.join(", ")
    )
}

fn locked(engine: &[&str], other: &[&str]) -> String {
    [
        "version = 4\n\n".to_owned(),
        package("velnor-gen", "0.1.0", false, &["engine"]),
        package("engine", "0.1.0", false, engine),
        package("other", "0.1.0", false, other),
        package("ext", "0.1.0", true, &[]),
        package("ext", "0.2.0", true, &[]),
    ]
    .concat()
}

fn listing(entry: &str) -> String {
    format!("{entry} {CRATES_IO}\n")
}

fn both() -> String {
    listing("ext 0.1.0") + &listing("ext 0.2.0")
}

fn viewing(lock: &str, list: Option<&str>) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = world()?;
    put(&mut view, "Cargo.lock", lock)?;
    if let Some(list) = list {
        put(&mut view, EXTERNALS, list)?;
    }
    Ok(view)
}

#[test]
fn a_registry_package_the_generator_reaches_must_be_on_the_externals_list() -> TestResult {
    let lock = locked(&["ext 0.2.0"], &[]);
    assert!(runs_every(&viewing(&lock, None)?, &QUIET)?, "no list");
    assert!(
        runs_every(&viewing(&lock, Some(""))?, &QUIET)?,
        "empty list"
    );
    assert!(
        runs_every(&viewing(&lock, Some(&listing("ext 0.1.0")))?, &QUIET)?,
        "another version"
    );
    assert!(
        runs_every(&viewing(&lock, Some("ext\n"))?, &QUIET)?,
        "a name without a version"
    );
    assert!(
        runs_every(&viewing(&lock, Some("ext 0.2.0\n"))?, &QUIET)?,
        "a line without a source"
    );
    assert!(
        skips_every(&viewing(&lock, Some(&listing("ext 0.2.0")))?, &QUIET)?,
        "the reached version"
    );
    assert!(
        skips_every(&viewing(&lock, Some(&both()))?, &QUIET)?,
        "an entry the closure no longer needs"
    );
    Ok(())
}

#[test]
fn a_registry_package_reached_through_any_depth_or_by_name_alone_is_listed_too() -> TestResult {
    let unversioned = "version = 4\n\n".to_owned()
        + &package("velnor-gen", "0.1.0", false, &["engine"])
        + &package("engine", "0.1.0", false, &["ext"])
        + &package("ext", "0.2.0", true, &[]);
    assert!(runs_every(&viewing(&unversioned, None)?, &QUIET)?);
    assert!(skips_every(
        &viewing(&unversioned, Some(&listing("ext 0.2.0")))?,
        &QUIET
    )?);
    Ok(())
}

#[test]
fn a_registry_package_only_another_package_uses_does_not_need_listing() -> TestResult {
    let lock = locked(&[], &["ext 0.2.0"]);
    assert!(skips_every(&viewing(&lock, None)?, &QUIET)?);
    Ok(())
}
