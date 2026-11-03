// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_actions_orchestrator::{OrchestratorError, prepare};

use super::scoped_workspace::{
    TestResult, commit, generate_lockfile, package_manifest, put, scoped_workspace,
};

const ACCEPTED_GLOB: &str = "notes/**";

fn config(glob: &str, tail: &str) -> String {
    format!("\n[stacks.rust]\nnon_inputs = [\"{glob}\"]\n{tail}")
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
    assert_rejected_only_for_overlap("lockfile", "*.lock", "", nothing)
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
    for glob in ["rust-toolchain.toml", "mise.toml"] {
        assert_rejected_only_for_overlap("tool_file", glob, "", |root| {
            put(
                root,
                "rust-toolchain.toml",
                "[toolchain]\nchannel = \"1.99.1\"\n",
            )?;
            put(root, "mise.toml", "[tools]\n")
        })?;
    }
    Ok(())
}
