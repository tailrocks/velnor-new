// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use velnor_model::{PolicyTools, RepoPath};
use velnor_rust::policy_runs;

use super::events::DENY_MISE;
use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

#[test]
fn the_policy_lane_installs_the_helper_it_plans_and_runs_with() -> TestResult {
    let mut multi = view(&["Cargo.toml"], None)?;
    for (path, text) in [("deny.toml", ""), ("mise.toml", DENY_MISE)] {
        let path = RepoPath::new(path)?;
        multi.tracked_paths.insert(path.clone());
        multi.files.insert(path, text.as_bytes().to_vec());
    }
    let lock = RepoPath::new(".velnor/rust-ci.lock")?;
    multi.tracked_paths.insert(lock.clone());
    multi.files.insert(
        lock,
        String::from_utf8(std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../samples/helper-lock/view/.velnor/rust-ci.lock"),
        )?)?
        .replace("0.1.79", "0.1.81")
        .into_bytes(),
    );
    let text = workflow(&multi)?;
    assert!(text.contains("Install velnor-ci"), "{text}");
    let policy = text.split("\n  policy:\n").nth(1).unwrap_or_default();
    let policy = policy.split("\n  required:\n").next().unwrap_or_default();
    assert!(policy.contains("cargo-deny"), "{policy}");
    assert!(policy.contains("Install velnor-ci"), "{policy}");
    Ok(())
}

#[test]
fn a_license_text_change_runs_the_policy_lane_only_with_cargo_deny() -> TestResult {
    let changed = |path: &str| -> Result<BTreeSet<RepoPath>, Box<dyn std::error::Error>> {
        Ok(BTreeSet::from([RepoPath::new(path)?]))
    };
    let deny = PolicyTools {
        deny: true,
        machete: false,
    };
    let machete = PolicyTools {
        deny: false,
        machete: true,
    };
    for path in [
        "LICENSE",
        "LICENSE-MIT",
        "crates/a/LICENSE.md",
        "licence.txt",
    ] {
        assert!(
            policy_runs(deny, &changed(path)?, false, true).deny,
            "{path}"
        );
        assert!(
            !policy_runs(machete, &changed(path)?, false, true).deny,
            "{path}"
        );
    }
    assert!(!policy_runs(deny, &changed("docs/readme.md")?, false, true).deny);
    Ok(())
}

#[test]
fn the_policy_lane_checks_every_cargo_root_in_its_own_folder() -> TestResult {
    let mut view = view(&["Cargo.toml", "tools/Cargo.toml"], None)?;
    let deny = RepoPath::new("deny.toml")?;
    view.tracked_paths.insert(deny.clone());
    view.files.insert(deny, Vec::new());
    view.files
        .insert(RepoPath::new("mise.toml")?, DENY_MISE.as_bytes().to_vec());
    let text = workflow(&view)?;
    let policy = text
        .split("\n  policy:\n")
        .nth(1)
        .ok_or("no policy job")?
        .split("\n  required:\n")
        .next()
        .ok_or("no policy job end")?;
    assert_eq!(policy.matches("Dependency policy").count(), 2, "{policy}");
    assert!(policy.contains("cargo-deny check"), "{policy}");
    assert!(
        policy.contains("cargo-deny --manifest-path ''tools/Cargo.toml'' check"),
        "{policy}"
    );
    Ok(())
}

#[test]
fn the_policy_lane_runs_machete_once_from_the_repository_root() -> TestResult {
    let mut view = view(&["Cargo.toml", "tools/Cargo.toml"], None)?;
    let mise = RepoPath::new("mise.toml")?;
    view.files.insert(
        mise,
        b"[tools]\nrust = \"1.99.1\"\n\"aqua:nextest-rs/nextest/cargo-nextest\" = \"0.9.151\"\ncargo-machete = \"0.9.1\"\n"
            .to_vec(),
    );
    let text = workflow(&view)?;
    let policy = text
        .split("\n  policy:\n")
        .nth(1)
        .ok_or("no policy job")?
        .split("\n  required:\n")
        .next()
        .ok_or("no policy job end")?;
    assert_eq!(policy.matches("cargo-machete").count(), 1, "{policy}");
    assert!(!policy.contains("working-directory"), "{policy}");
    Ok(())
}
