//! T20 tofu determinism and read-only atomicity: fixed-point, cross-checkout, snapshots.
//!
//! Split from `impl_tofu_t20` under the 400-line gate; fixtures live
//! there, determinism and read-only proofs live here.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;

use tempfile::TempDir;
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};

use super::impl_common::{TestResult, plan_for, snapshot};
use super::impl_tofu_t20::pure_tofu_repo;

/// Stale lock bytes (T14-proven non-blocking through generate).
const STALE_LOCK: &str = "provider \"example.com/a/b\" {\nversion = \"1.0.0\"\n}\n";

/// Second tofu generate changes no byte; the staged validators are recorded.
#[test]
fn tofu_generate_is_fixed_point_and_validated() -> TestResult {
    let repo = pure_tofu_repo(
        &["stacks/a"],
        &[("stacks/a/.terraform.lock.hcl", STALE_LOCK)],
    )?;
    let root = repo.path();
    let prep = prepare(root)?;
    generate(&prep, &GenerateOptions { output_dir: None })?;
    let first: BTreeMap<String, Vec<u8>> = snapshot(root)?
        .into_iter()
        .map(|(rel, (bytes, _))| (rel, bytes))
        .collect();
    let prep = prepare(root)?;
    let report = generate(&prep, &GenerateOptions { output_dir: None })?;
    assert_eq!(
        report.validated_by,
        vec![
            "actionlint@1.7.12".to_owned(),
            "shellcheck@0.11.0".to_owned(),
            "zizmor@1.30.1".to_owned(),
        ],
        "tofu steps pass the staged validators: {:?}",
        report.validated_by
    );
    let second: BTreeMap<String, Vec<u8>> = snapshot(root)?
        .into_iter()
        .map(|(rel, (bytes, _))| (rel, bytes))
        .collect();
    assert_eq!(first, second, "second generate changes no byte");
    Ok(())
}

/// Identical tofu inputs at different absolute paths stage identical bytes.
#[test]
fn tofu_cross_checkout_determinism() -> TestResult {
    let left = pure_tofu_repo(&["stacks/a", "stacks/b"], &[])?;
    let right = pure_tofu_repo(&["stacks/a", "stacks/b"], &[])?;
    assert_ne!(
        left.path().canonicalize()?,
        right.path().canonicalize()?,
        "distinct checkouts"
    );
    let mut trees = BTreeSet::new();
    for repo in [&left, &right] {
        let prep = prepare(repo.path())?;
        let parent = TempDir::new()?;
        let preview_root = parent.path().join("preview");
        let report = generate(
            &prep,
            &GenerateOptions {
                output_dir: Some(preview_root.clone()),
            },
        )?;
        let mut files = BTreeMap::new();
        for rel in &report.files_written {
            files.insert(rel.clone(), fs::read(preview_root.join(rel))?);
        }
        let names: Vec<&str> = files.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            [
                ".github/AGENTS.md",
                ".github/actionlint.yaml",
                ".github/actions/tofu-provider-admission/action.yml",
                ".github/actions/u26/action.yml",
                ".github/actions/velnor-tool-seed/action.yml",
                ".github/actions/velnor-tools-cache-restore/action.yml",
                ".github/actions/velnor-tools-prelude-u26/action.yml",
                ".github/scripts/velnor-tools-cache-identity.sh",
                ".github/workflows/ci.yml",
            ],
            "tofu workflows compose with generated agent docs (PR #11)"
        );
        trees.insert(format!("{files:?}"));
    }
    assert_eq!(trees.len(), 1, "staged bytes agree across checkouts");
    Ok(())
}

/// Plan and generate leave every tofu byte alone; only `.github` is written.
#[test]
fn tofu_plan_and_generate_leave_every_byte() -> TestResult {
    let repo = pure_tofu_repo(
        &["stacks/a"],
        &[
            ("stacks/a/.terraform.lock.hcl", STALE_LOCK),
            ("stacks/a/.terraform/modules/x", "cached\n"),
            ("stacks/a/custom.tfvars", "x = 1\n"),
        ],
    )?;
    let root = repo.path();
    let before = snapshot(root)?;
    let prep = prepare(root)?;
    let _ = plan_for(&prep)?;
    let preview_parent = TempDir::new()?;
    generate(
        &prep,
        &GenerateOptions {
            output_dir: Some(preview_parent.path().join("preview")),
        },
    )?;
    assert_eq!(before, snapshot(root)?, "preview writes nothing");
    generate(&prep, &GenerateOptions { output_dir: None })?;
    let after = snapshot(root)?;
    let mut added = Vec::new();
    for (rel, (bytes, _)) in &after {
        match before.get(rel) {
            Some((old, _)) => assert_eq!(old, bytes, "{rel} byte-identical"),
            None => added.push(rel.clone()),
        }
    }
    assert!(
        added.iter().all(|rel| rel.starts_with(".github/")),
        "only .github gains files: {added:?}"
    );
    assert_eq!(
        fs::read(root.join("stacks/a/.terraform.lock.hcl"))?,
        STALE_LOCK.as_bytes().to_vec(),
        "lock untouched"
    );
    assert_eq!(
        fs::read(root.join("stacks/a/.terraform/modules/x"))?,
        b"cached\n",
        "workdir cache untouched"
    );
    Ok(())
}
