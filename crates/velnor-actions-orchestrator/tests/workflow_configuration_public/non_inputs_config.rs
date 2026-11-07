// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_actions_orchestrator::{OrchestratorError, prepare};

use super::scoped_workspace::{
    A5_COMPLETE_RULE, TestResult, commit, generate_lockfile, package_manifest, plan, put,
    scoped_workspace,
};
use super::workflow_configuration_fixture::make_repo;

const ACCEPTED_GLOB: &str = "notes/**";

const NON_RUST_CHECK: &str = "\n[[checks]]\nid = \"docs-lint\"\ntask = \"fixture-check\"\ninputs = [\"scripts/lint.sh\"]\ntools = []\n[checks.runner]\nlabel = \"ubuntu-26.04\"\nplatform = \"linux_x64\"\nexecutor = \"hosted\"\n";

fn config(glob: &str, tail: &str) -> String {
    format!("\n[stacks.rust]\nnon_inputs = [\"{glob}\"]\n{A5_COMPLETE_RULE}{tail}")
}

fn prepared(
    glob: &str,
    tail: &str,
    mutate: fn(&Path) -> TestResult,
) -> Result<Result<(), OrchestratorError>, Box<dyn Error>> {
    let repo = scoped_workspace(&config(glob, tail))?;
    mutate(repo.root())?;
    commit(repo.root(), "scenario")?;
    Ok(prepare(repo.root()).map(|_| ()))
}

fn assert_rejected_only_for_overlap(
    kind: &str,
    glob: &str,
    tail: &str,
    mutate: fn(&Path) -> TestResult,
) -> TestResult {
    match prepared(glob, tail, mutate)? {
        Err(OrchestratorError::Config {
            key_path, problem, ..
        }) => {
            assert_eq!(key_path, "stacks.rust.non_inputs");
            assert_eq!(problem, format!("non_input_overlaps_claim:{kind}:{glob}"));
        }
        other => {
            return Err(format!("{kind}: expected the named config error, got {other:?}").into());
        }
    }
    let accepted = prepared(ACCEPTED_GLOB, tail, mutate)?;
    assert!(
        accepted.is_ok(),
        "{kind}: disjoint glob rejected: {accepted:?}"
    );
    Ok(())
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "must match the scenario mutator signature"
)]
fn nothing(_: &Path) -> TestResult {
    Ok(())
}

#[test]
fn non_input_overlapping_package_dir_is_rejected() -> TestResult {
    assert_rejected_only_for_overlap("package_dir", "docs/**", "", |root| {
        let manifest = fs::read_to_string(root.join("Cargo.toml"))?;
        put(
            root,
            "Cargo.toml",
            &manifest.replace("members = [", "members = [\"docs/tool\", "),
        )?;
        let (path, body) = package_manifest("docs/tool", "tool", "");
        put(root, &path, &body)?;
        put(root, "docs/tool/src/lib.rs", "pub fn f() {}\n")?;
        generate_lockfile(root)
    })
}

#[test]
fn non_input_overlapping_declared_input_is_rejected() -> TestResult {
    assert_rejected_only_for_overlap(
        "declared_input",
        "docs/proposed/**",
        "\n[[stacks.rust.inputs]]\nglobs = [\"docs/proposed/opentofu-goldens/**\"]\npackages = [\"a8\"]\n",
        |root| put(root, "docs/proposed/opentofu-goldens/case.txt", "oracle\n"),
    )?;
    assert_rejected_only_for_overlap(
        "declared_input",
        "docs/**",
        "\n[[stacks.rust.inputs]]\nglobs = [\"docs/schema/**\"]\npackages = [\"a8\"]\n",
        |root| put(root, "docs/schema/x.sql", "select 1;\n"),
    )
}

#[test]
fn non_input_overlapping_read_target_is_rejected() -> TestResult {
    assert_rejected_only_for_overlap("read_target", "docs/**", "", |root| {
        put(
            root,
            "apps/a7/src/lib.rs",
            "pub const D: &str = include_str!(\"../../../docs/x.md\");\n",
        )?;
        put(root, "docs/x.md", "x\n")
    })
}

#[cfg(unix)]
#[test]
fn non_input_overlapping_symlink_is_rejected() -> TestResult {
    assert_rejected_only_for_overlap("symlink", "docs/**", "", |root| {
        fs::create_dir_all(root.join("links"))?;
        std::os::unix::fs::symlink("../docs/guide.md", root.join("links/guide.md"))?;
        Ok(())
    })
}

#[test]
fn non_input_overlapping_lockfile_is_rejected() -> TestResult {
    assert_rejected_only_for_overlap("lockfile", "tools/**", "", |root| {
        put(root, "tools/Cargo.lock", "version = 4\n")
    })
}

#[test]
fn non_input_overlapping_github_is_rejected() -> TestResult {
    assert_rejected_only_for_overlap("github", ".github/workflows/**", "", nothing)
}

#[test]
fn non_input_overlapping_velnor_is_rejected() -> TestResult {
    assert_rejected_only_for_overlap("velnor", ".velnor/**", "", nothing)
}

#[test]
fn non_input_overlapping_tool_file_is_rejected() -> TestResult {
    assert_rejected_only_for_overlap("tool_file", ".config/**", "", |root| {
        put(root, ".config/mise.toml", "[tools]\n")
    })
}

const NON_INPUTS_KEY: &str = "stacks.rust.non_inputs";

const A8_ESCAPE: &str = "pub fn d() -> std::path::PathBuf {\n    std::path::Path::new(env!(\"CARGO_MANIFEST_DIR\")).join(\"../../docs/x\")\n}\n";

const A8_OUTSIDE: &str = "pub const D: &str = include_str!(\"/srv/docs/x.md\");\n";

const A8_COMPLETE_RULE: &str =
    "\n[[stacks.rust.inputs]]\nglobs = [\"apps/a8/**\"]\npackages = [\"a8\"]\ncomplete = true\n";

fn assert_problem(
    label: &str,
    result: Result<(), OrchestratorError>,
    expected: &str,
) -> TestResult {
    match result {
        Err(OrchestratorError::Config {
            key_path, problem, ..
        }) => {
            assert_eq!(key_path, NON_INPUTS_KEY, "{label}");
            assert_eq!(problem, expected, "{label}");
            Ok(())
        }
        other => Err(format!("{label}: expected {expected}, got {other:?}").into()),
    }
}

fn prepared_with(
    config_text: &str,
    files: &[(&str, &str)],
) -> Result<Result<(), OrchestratorError>, Box<dyn Error>> {
    let repo = scoped_workspace(config_text)?;
    for (path, body) in files {
        put(repo.root(), path, body)?;
    }
    commit(repo.root(), "scenario")?;
    Ok(prepare(repo.root()).map(|_| ()))
}

fn assert_a8_read_forces_the_package_without_non_inputs() -> TestResult {
    let repo = scoped_workspace(&format!("\n[stacks.rust]\n{A5_COMPLETE_RULE}"))?;
    put(repo.root(), "apps/a8/src/lib.rs", A8_ESCAPE)?;
    let base = commit(repo.root(), "a8 reads the docs tree")?;
    put(repo.root(), "apps/a1/src/lib.rs", "pub fn g() {}\n")?;
    let tip = commit(repo.root(), "change a1")?;
    let run = plan(repo.root(), "pull_request", Some(&base), &tip)?;
    let warnings = run.warnings();
    assert!(
        warnings
            .iter()
            .any(|warning| warning.starts_with("undeclared_reads:a8:")),
        "{warnings:?}"
    );
    Ok(())
}

#[test]
fn non_inputs_unsupported_while_a_read_cannot_be_bounded() -> TestResult {
    assert_problem(
        "a5 without a complete rule",
        prepared_with("\n[stacks.rust]\nnon_inputs = [\"docs/**\"]\n", &[])?,
        "non_inputs_unsupported:unbounded_read:a5",
    )?;
    let bounded = prepared_with(&config("docs/**", ""), &[])?;
    assert!(bounded.is_ok(), "a5 with a complete rule: {bounded:?}");
    assert_problem(
        "a8 reads the docs tree through the manifest directory",
        prepared_with(&config("docs/**", ""), &[("apps/a8/src/lib.rs", A8_ESCAPE)])?,
        "non_inputs_unsupported:unbounded_read:a8",
    )?;
    assert_problem(
        "a8 includes an absolute path",
        prepared_with(
            &config("docs/**", ""),
            &[("apps/a8/src/lib.rs", A8_OUTSIDE)],
        )?,
        "non_inputs_unsupported:outside_read:a8",
    )?;
    assert_a8_read_forces_the_package_without_non_inputs()?;
    let bounded = prepared_with(
        &config("docs/**", A8_COMPLETE_RULE),
        &[("apps/a8/src/lib.rs", A8_OUTSIDE)],
    )?;
    assert!(bounded.is_ok(), "a8 with a complete rule: {bounded:?}");
    Ok(())
}

#[test]
fn non_inputs_unsupported_for_a_check_without_rust_input() -> TestResult {
    let files = [
        ("mise.toml", "[tasks.fixture-check]\nrun = \"true\"\n"),
        ("scripts/lint.sh", "#!/bin/sh\n"),
    ];
    assert_problem(
        "check reads only a script",
        prepared_with(&config("docs/**", NON_RUST_CHECK), &files)?,
        "non_inputs_unsupported:non_rust_check:docs-lint",
    )?;
    let rust_check = NON_RUST_CHECK.replace("scripts/lint.sh", "apps/a1/Cargo.toml");
    let rust = prepared_with(&config("docs/**", &rust_check), &files)?;
    assert!(rust.is_ok(), "check with a Rust input: {rust:?}");
    let without = prepared_with(
        &format!("\n[stacks.rust]\n{A5_COMPLETE_RULE}{NON_RUST_CHECK}"),
        &files,
    )?;
    assert!(
        without.is_ok(),
        "same check without non_inputs: {without:?}"
    );
    Ok(())
}

#[test]
fn non_inputs_unsupported_for_a_root_package() -> TestResult {
    let scenario = |tail: &str| -> Result<Result<(), OrchestratorError>, Box<dyn Error>> {
        let repo = make_repo(&format!(
            "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n{tail}"
        ))?;
        put(repo.path(), "docs/a.md", "a\n")?;
        commit(repo.path(), "root package")?;
        Ok(prepare(repo.path()).map(|_| ()))
    };
    assert_problem(
        "root package with non_inputs",
        scenario("\n[stacks.rust]\nnon_inputs = [\"docs/**\"]\n")?,
        "non_inputs_unsupported:root_package",
    )?;
    let without = scenario("\n[stacks.rust]\n")?;
    assert!(
        without.is_ok(),
        "root package without non_inputs: {without:?}"
    );
    Ok(())
}
