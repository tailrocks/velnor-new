// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::scoped_workspace::{
    A5_COMPLETE_RULE, CORE_CLOSURE, FORCED_AND_DEPENDENTS, TestResult, commit, generate_lockfile,
    plan, put, scoped_workspace, union,
};

fn repository_root() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?)
}

#[test]
fn oracle_change_selects_archive_guard_and_dependents() -> TestResult {
    let root = repository_root()?;
    let config: toml::Value =
        toml::from_str(&std::fs::read_to_string(root.join(".velnor/config.toml"))?)?;
    let rules = config["stacks"]["rust"]["inputs"]
        .as_array()
        .ok_or("missing Rust inputs")?;
    let owner = rules
        .iter()
        .find(|rule| {
            rule["packages"].as_array().is_some_and(|packages| {
                packages
                    .iter()
                    .any(|package| package.as_str() == Some("velnor-archive-guard"))
            })
        })
        .ok_or("missing archive guard inputs")?;
    let globs = owner["globs"]
        .as_array()
        .ok_or("missing archive guard globs")?
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let fixture_config = format!(
        "\n[stacks.rust]\nnon_inputs = [\"docs/proposed/**\"]\n[[stacks.rust.inputs]]\nglobs = [{globs}]\npackages = [\"velnor-archive-guard\"]\ncomplete = true\n{A5_COMPLETE_RULE}"
    );
    let repo = scoped_workspace(&fixture_config)?;
    std::fs::create_dir_all(repo.root().join("crates"))?;
    std::fs::rename(
        repo.root().join("libs/core"),
        repo.root().join("crates/velnor-archive-guard"),
    )?;
    for path in [
        "Cargo.toml",
        "crates/velnor-archive-guard/Cargo.toml",
        "apps/a1/Cargo.toml",
        "apps/a3/Cargo.toml",
        "apps/a4/Cargo.toml",
        "apps/a5/Cargo.toml",
        "apps/a6/Cargo.toml",
    ] {
        let source = std::fs::read_to_string(repo.root().join(path))?;
        let updated = source
            .replace("libs/core", "crates/velnor-archive-guard")
            .replace("name = \"core\"", "name = \"velnor-archive-guard\"")
            .replace(
                "core = { path",
                "core = { package = \"velnor-archive-guard\", path",
            );
        put(repo.root(), path, &updated)?;
    }
    generate_lockfile(repo.root())?;
    let oracle = "fixtures/opentofu-goldens/cases/dogfood/plan.txt";
    put(repo.root(), oracle, "original\n")?;
    let base = commit(repo.root(), "base oracle")?;
    put(repo.root(), oracle, "changed\n")?;
    let head = commit(repo.root(), "change oracle")?;
    let run = plan(repo.root(), "pull_request", Some(&base), &head)?;
    let mut expected = union(&[&CORE_CLOSURE, &FORCED_AND_DEPENDENTS]);
    expected.remove("core");
    expected.insert("velnor-archive-guard".to_owned());
    assert_eq!(run.with_disposition("run")?, expected);
    assert_eq!(
        run.jobs()?.get("rust-a8").map(String::as_str),
        Some("not_affected")
    );
    let warnings = run.warnings();
    assert!(
        warnings
            .iter()
            .any(|warning| warning
                == "undeclared_reads:a7:apps/a7/src/lib.rs:reads:apps/a1/data.txt")
    );
    assert!(
        warnings
            .iter()
            .any(|warning| warning == "undeclared_reads:util:build_script")
    );
    assert!(
        !warnings
            .iter()
            .any(|warning| warning == "unclassified_files:all_changed")
    );
    Ok(())
}

#[test]
fn inventory_generator_changes_select_its_cli_owner() -> TestResult {
    const SCRIPT: &str = "scripts/native-image-candidate-inventory.py";
    let root = repository_root()?;
    let config: toml::Value =
        toml::from_str(&std::fs::read_to_string(root.join(".velnor/config.toml"))?)?;
    let rules = config["stacks"]["rust"]["inputs"]
        .as_array()
        .ok_or("missing Rust input rules")?;
    let owner = rules
        .iter()
        .find(|rule| {
            rule["packages"].as_array().is_some_and(|packages| {
                packages
                    .iter()
                    .any(|package| package.as_str() == Some("velnor-actions-cli"))
            })
        })
        .ok_or("missing CLI owner rule")?;
    let globs = owner["globs"].as_array().ok_or("missing CLI owner globs")?;
    assert!(globs.iter().any(|glob| glob.as_str() == Some(SCRIPT)));
    let fixture_config = format!(
        "\n[[stacks.rust.inputs]]\nglobs = [\"{SCRIPT}\"]\npackages = [\"velnor-actions-cli\"]\ncomplete = true\n"
    );
    let repo = scoped_workspace(&fixture_config)?;
    put(
        repo.root(),
        "apps/a5/Cargo.toml",
        "[package]\nname = \"velnor-actions-cli\"\nversion = \"0.1.0\"\nedition = \"2021\"\n[target.'cfg(unix)'.dependencies]\ncore = { path = \"../../libs/core\" }\n",
    )?;
    generate_lockfile(repo.root())?;
    put(repo.root(), SCRIPT, "initial\n")?;
    let base = commit(repo.root(), "base generator")?;
    put(repo.root(), SCRIPT, "changed\n")?;
    let head = commit(repo.root(), "change generator")?;
    let run = plan(repo.root(), "pull_request", Some(&base), &head)?;
    assert_eq!(
        run.jobs()?
            .get("rust-velnor-actions-cli")
            .map(String::as_str),
        Some("run")
    );
    assert_eq!(
        run.jobs()?.get("rust-a1").map(String::as_str),
        Some("not_affected")
    );
    Ok(())
}
