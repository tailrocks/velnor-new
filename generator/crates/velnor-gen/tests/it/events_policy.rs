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
    assert!(
        policy.contains("cargo-deny --config ''deny.toml'' check"),
        "{policy}"
    );
    assert!(
        policy.contains(
            "cargo-deny --manifest-path ''tools/Cargo.toml'' --config ''deny.toml'' check"
        ),
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

#[test]
fn cargo_deny_configs_enable_the_policy_lane_and_require_a_pin() -> TestResult {
    for (manifest, config) in [
        ("Cargo.toml", "deny.toml"),
        ("Cargo.toml", ".deny.toml"),
        ("Cargo.toml", ".cargo/deny.toml"),
        ("Cargo.toml", ".config/deny.toml"),
        ("lab/Cargo.toml", "lab/.cargo/deny.toml"),
        ("lab/Cargo.toml", ".config/deny.toml"),
    ] {
        let mut repo = view(&[manifest], None)?;
        repo.tracked_paths.insert(RepoPath::new(config)?);
        let error = workflow(&repo).err().ok_or("missing pin accepted")?;
        assert!(
            error.to_string().contains("cargo-deny"),
            "{config}: {error}"
        );
        repo.files
            .insert(RepoPath::new("mise.toml")?, DENY_MISE.as_bytes().to_vec());
        let text = workflow(&repo)?;
        assert!(text.contains("cargo-deny"), "{config}: {text}");
        if manifest.starts_with("lab/") {
            assert!(
                text.contains("cargo-deny --manifest-path ''lab/Cargo.toml'' --config"),
                "{text}"
            );
        }
    }
    for config in [
        "docs/deny.toml",
        "lab/other/deny.toml",
        ".cargo/deny.toml.extra",
    ] {
        let mut repo = view(&["lab/Cargo.toml"], None)?;
        repo.tracked_paths.insert(RepoPath::new(config)?);
        repo.files
            .insert(RepoPath::new("mise.toml")?, DENY_MISE.as_bytes().to_vec());
        assert!(!workflow(&repo)?.contains("cargo-deny"), "{config}");
    }
    Ok(())
}

#[test]
fn changes_to_cargo_deny_configs_select_policy() -> TestResult {
    let tools = PolicyTools {
        deny: false,
        machete: false,
    };
    for name in [
        "deny.toml",
        ".deny.toml",
        ".cargo/deny.toml",
        ".config/deny.toml",
        "lab/.cargo/deny.toml",
    ] {
        let changed = BTreeSet::from([RepoPath::new(name)?]);
        assert!(policy_runs(tools, &changed, false, true).deny, "{name}");
    }
    for name in [
        "deny.toml.extra",
        ".deny.toml.bak",
        ".cargo/deny.toml.extra",
    ] {
        let changed = BTreeSet::from([RepoPath::new(name)?]);
        assert!(!policy_runs(tools, &changed, false, true).deny, "{name}");
    }
    Ok(())
}

#[test]
fn each_cargo_root_uses_its_nearest_deny_config_with_stable_precedence() -> TestResult {
    let mut repo = view(&["Cargo.toml", "lab/Cargo.toml", "other/Cargo.toml"], None)?;
    for config in [
        "deny.toml",
        ".config/deny.toml",
        "lab/.config/deny.toml",
        "lab/.deny.toml",
    ] {
        repo.tracked_paths.insert(RepoPath::new(config)?);
    }
    repo.files
        .insert(RepoPath::new("mise.toml")?, DENY_MISE.as_bytes().to_vec());
    let text = workflow(&repo)?;
    assert!(
        text.contains("cargo-deny --config ''deny.toml'' check"),
        "{text}"
    );
    assert!(
        text.contains(
            "cargo-deny --manifest-path ''lab/Cargo.toml'' --config ''lab/.deny.toml'' check"
        ),
        "{text}"
    );
    assert!(
        text.contains(
            "cargo-deny --manifest-path ''other/Cargo.toml'' --config ''deny.toml'' check"
        ),
        "{text}"
    );

    repo.tracked_paths.remove(&RepoPath::new("lab/.deny.toml")?);
    let text = workflow(&repo)?;
    assert!(
        text.contains(
            "cargo-deny --manifest-path ''lab/Cargo.toml'' --config ''lab/.config/deny.toml'' check"
        ),
        "{text}"
    );
    Ok(())
}

#[test]
fn a_cargo_root_without_a_config_keeps_the_default_command() -> TestResult {
    let mut repo = view(&["lab/Cargo.toml", "other/Cargo.toml"], None)?;
    repo.tracked_paths
        .insert(RepoPath::new("lab/.config/deny.toml")?);
    repo.files
        .insert(RepoPath::new("mise.toml")?, DENY_MISE.as_bytes().to_vec());
    let text = workflow(&repo)?;
    assert!(
        text.contains("cargo-deny --manifest-path ''other/Cargo.toml'' check"),
        "{text}"
    );
    assert!(
        text.contains(
            "cargo-deny --manifest-path ''lab/Cargo.toml'' --config ''lab/.config/deny.toml'' check"
        ),
        "{text}"
    );
    Ok(())
}

#[test]
fn a_deny_config_with_a_quote_is_passed_as_one_checkout_relative_argument() -> TestResult {
    let mut repo = view(&["lab's/Cargo.toml"], None)?;
    repo.tracked_paths
        .insert(RepoPath::new("lab's/.config/deny.toml")?);
    repo.files
        .insert(RepoPath::new("mise.toml")?, DENY_MISE.as_bytes().to_vec());
    let text = workflow(&repo)?;
    let parsed = serde_saphyr::from_str::<serde_json::Value>(&text)?;
    let steps = parsed["jobs"]["policy"]["steps"]
        .as_array()
        .ok_or("no policy steps")?;
    assert!(
        steps
            .iter()
            .all(|step| step.get("working-directory").is_none())
    );
    assert!(steps.iter().any(|step| step["run"].as_str() == Some(r#". "$VELNOR_EXEC" run -- cargo-deny --manifest-path 'lab'\''s/Cargo.toml' --config 'lab'\''s/.config/deny.toml' check"#)));
    Ok(())
}
