// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RepoFacts;
use velnor_rust::detect;

use crate::facts::{TestResult, lib, path, repo};
use crate::weaken_scope::{introduced_files, named};

const CONFIG: &str = ".velnor/rust-ci.toml";
const CARGO_CONFIG: &str = ".cargo/config.toml";
const TOOLCHAIN: &str = "[toolchain]\nchannel = \"1.99.1\"\ntargets = [\"x86_64-unknown-linux-gnu\", \"aarch64-unknown-linux-gnu\"]\n";
const RUNNER_B: &str = "[target.x86_64-unknown-linux-gnu]\nrunner = \"true\"\n";

#[test]
fn doc_false_on_a_test_bench_or_example_removes_nothing_from_cargo_doc() -> TestResult {
    let manifest = "[package]\nname = \"foo\"\n\n[[test]]\nname = \"t\"\ndoc = false\n\n[[bench]]\nname = \"b\"\ndoc = false\n\n[[example]]\nname = \"e\"\ndoc = false\n\n[[bin]]\nname = \"x\"\ndoc = false\n";
    let named = named(&[("crates/foo/Cargo.toml", manifest)], false)?;
    assert_eq!(
        named,
        [
            "crates/foo/Cargo.toml: [[bin]] name = \"x\" doc = false: leaves the bin target x of foo out of cargo doc"
        ]
    );
    Ok(())
}

#[test]
fn reordering_the_packages_of_an_input_rule_is_not_new() -> TestResult {
    let rule =
        |packages: &str| format!("[[inputs]]\nglob = \"data/**\"\npackages = [{packages}]\n");
    let head = rule("\"a\", \"b\"");
    let facts = facts(
        &[(CONFIG, &head)],
        &[("a", "crates/a/Cargo.toml"), ("b", "crates/b/Cargo.toml")],
    )?;
    let base = rule("\"b\", \"a\", \"b\"");
    assert_eq!(
        introduced_files(&facts, &[CONFIG], &[(CONFIG, &base)])?,
        Vec::<String>::new()
    );
    Ok(())
}

#[test]
fn switching_build_target_to_a_triple_that_has_a_runner_is_new() -> TestResult {
    let with_target = |triple: &str| format!("[build]\ntarget = \"{triple}\"\n\n{RUNNER_B}");
    let head = with_target("x86_64-unknown-linux-gnu");
    let base = with_target("aarch64-unknown-linux-gnu");
    let facts = facts(
        &[(CARGO_CONFIG, &head), ("rust-toolchain.toml", TOOLCHAIN)],
        &[("a", "crates/a/Cargo.toml")],
    )?;
    assert_eq!(
        introduced_files(&facts, &[CARGO_CONFIG], &[(CARGO_CONFIG, &base)])?,
        [CARGO_CONFIG]
    );
    let unchanged = facts_unchanged(&head)?;
    assert_eq!(unchanged, Vec::<String>::new());
    Ok(())
}

fn facts_unchanged(text: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let facts = facts(
        &[(CARGO_CONFIG, text), ("rust-toolchain.toml", TOOLCHAIN)],
        &[("a", "crates/a/Cargo.toml")],
    )?;
    introduced_files(&facts, &[CARGO_CONFIG], &[(CARGO_CONFIG, text)])
}

fn facts(
    files: &[(&str, &str)],
    members: &[(&str, &str)],
) -> Result<RepoFacts, Box<dyn std::error::Error>> {
    let members = members
        .iter()
        .map(|(name, manifest)| lib(name, manifest))
        .collect::<Result<Vec<_>, _>>()?;
    let mut view = repo(vec![("Cargo.toml", members)], files)?;
    for (file, _) in files {
        view.tracked_paths.insert(path(file)?);
    }
    Ok(detect(&view)?)
}
