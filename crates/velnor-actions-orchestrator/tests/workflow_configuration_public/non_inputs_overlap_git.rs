// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;

use tempfile::TempDir;
use velnor_actions_orchestrator::{OrchestratorError, prepare};

use super::non_inputs_differential::{planner_set, sparse_set, tracked};
use super::non_inputs_grammar::{base_config, load};
use super::scoped_workspace::{A5_COMPLETE_RULE, TestResult, commit, put, scoped_workspace};
use super::workflow_configuration_fixture::make_repo;

fn light_repo(glob: &str) -> Result<TempDir, Box<dyn Error>> {
    let repo = make_repo(&format!(
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\n[stacks.rust]\nnon_inputs = [\"{glob}\"]\n"
    ))?;
    let root = repo.path();
    fs::remove_dir_all(root.join("src"))?;
    put(
        root,
        "Cargo.toml",
        "[workspace]\nmembers = [\"crates/demo\"]\nresolver = \"2\"\n",
    )?;
    put(
        root,
        "crates/demo/Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    put(root, "crates/demo/src/lib.rs", "pub fn f() {}\n")?;
    for path in [
        "docs/a.md",
        "docs/Sub/b.md",
        "Notes/n.md",
        "a-b/q.md",
        "apps/x.md",
    ] {
        put(root, path, "x\n")?;
    }
    commit(root, "light")?;
    Ok(repo)
}

#[test]
fn case_conflation_is_rejected_exactly_when_an_ignorecase_checkout_would_differ() -> TestResult {
    let base = base_config()?;
    let globs = [
        "Docs/**",
        "DOCS/**",
        "docs/**",
        "docs/sub/**",
        "Notes/**",
        "notes/**",
        "a-B/**",
        "docs/Sub/**",
        "docs/*/**",
        "apps/**",
        "APPS/**",
    ];
    let mut rejecting = BTreeSet::new();
    for glob in globs {
        let repo = light_repo(glob)?;
        let non_inputs = load(&base, &[glob])?.map_err(|error| error.to_string())?;
        let planner = planner_set(&tracked(repo.path())?, &non_inputs);
        let folded = sparse_set(repo.path(), &non_inputs, true)?;
        let expected_reject = planner != folded;
        if expected_reject {
            rejecting.insert(glob);
        }
        match (prepare(repo.path()), expected_reject) {
            (Err(OrchestratorError::Config { problem, .. }), true) => assert!(
                problem.starts_with("non_inputs_unsupported:case_conflation:"),
                "{glob}: {problem}"
            ),
            (Ok(_), false) => {}
            (Ok(_), true) => {
                return Err(format!("{glob}: expected case_conflation, got Ok").into());
            }
            (other, expected) => {
                return Err(format!(
                    "{glob}: expected reject={expected}, got {:?}",
                    other.map(|_| ())
                )
                .into());
            }
        }
    }
    for glob in ["Docs/**", "APPS/**", "docs/sub/**"] {
        assert!(rejecting.contains(glob), "{glob} must be a conflation");
    }
    for glob in ["docs/**", "Notes/**"] {
        assert!(!rejecting.contains(glob), "{glob} must not be a conflation");
    }
    Ok(())
}

const D3_HEADER: &str = "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n";

const D3_OK: [&str; 4] = ["docs/**", "notes/**", "scripts/**", "docs/*/**"];

const D3_CLAIMED_FILES: [&str; 8] = [
    "Cargo.toml",
    "Cargo.lock",
    "mise.toml",
    "rust-toolchain.toml",
    ".cargo/config.toml",
    ".github/workflows/ci.yml",
    ".velnor/config.toml",
    "README.md",
];

const D3_OVERLAPS: [(&str, Option<&str>); 13] = [
    ("apps/**", None),
    ("libs/**", None),
    ("apps/*/**", Some("package_dir")),
    ("libs/*/**", None),
    ("apps/a1/**", None),
    ("*/a1/**", None),
    ("*/src/**", None),
    ("*/*/**", None),
    ("*/**", None),
    (".github/**", Some("github")),
    (".velnor/**", None),
    (".cargo/**", Some("tool_file")),
    ("links/**", Some("symlink")),
];

#[test]
fn accepted_globs_never_exclude_a_claimed_path() -> TestResult {
    let repo = scoped_workspace("")?;
    let root = repo.root();
    for (path, body) in [
        (".cargo/config.toml", "[build]\n"),
        (".github/workflows/ci.yml", "name: x\n"),
        ("scripts/x.sh", "#!/bin/sh\n"),
        ("notes/n.md", "n\n"),
    ] {
        put(root, path, body)?;
    }
    fs::create_dir_all(root.join("links"))?;
    std::os::unix::fs::symlink("../README.md", root.join("links/readme"))?;
    commit(root, "claims")?;
    let base = base_config()?;
    let write = |glob: &str| -> TestResult {
        put(
            root,
            ".velnor/config.toml",
            &format!("{D3_HEADER}\n[stacks.rust]\nnon_inputs = [\"{glob}\"]\n{A5_COMPLETE_RULE}"),
        )
    };
    for glob in D3_OK {
        write(glob)?;
        let prepared = prepare(root);
        assert!(
            prepared.is_ok(),
            "{glob}: expected Ok, got {:?}",
            prepared.map(|_| ())
        );
        let non_inputs = load(&base, &[glob])?.map_err(|error| error.to_string())?;
        for path in sparse_set(root, &non_inputs, false)? {
            let claimed = path.starts_with("apps/")
                || path.starts_with("libs/")
                || path == "links/readme"
                || D3_CLAIMED_FILES.contains(&path.as_str());
            assert!(!claimed, "{glob}: the checkout drops the claimed {path}");
        }
    }
    for (glob, kind) in D3_OVERLAPS {
        write(glob)?;
        match prepare(root) {
            Err(OrchestratorError::Config { problem, .. }) => {
                let expected = kind.map_or_else(
                    || "non_input_overlaps_claim:".to_owned(),
                    |kind| format!("non_input_overlaps_claim:{kind}:{glob}"),
                );
                assert!(problem.starts_with(&expected), "{glob}: {problem}");
            }
            other => {
                return Err(format!(
                    "{glob}: expected non_input_overlaps_claim, got {:?}",
                    other.map(|_| ())
                )
                .into());
            }
        }
    }
    Ok(())
}
