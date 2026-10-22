//! Byte goldens for generated Bun workflows (hard decision 6).
//!
//! Each `fixtures/<case>/input` repository is copied into a fresh git
//! checkout and generated; its `.github/workflows/ci.yml` must byte-match
//! `fixtures/<case>/expected/.github/workflows/ci.yml`. The only
//! normalization is the generator release identity the synthetic test
//! manifest embeds (version, per-target digests, source marker), so a
//! release bump never churns the golden.
//!
//! `bun-workspace` declares no suite. It is the proof that repositories which
//! do not opt into suites, sharding or the newer Bun keys keep byte-identical
//! output: every change after it must leave it unchanged.
//!
//! `bun-workspace-suites` opts in (a sanitized copy of the `ChainArgos/platform`
//! configuration): it changes only when a suite feature changes on purpose.
//!
//! Regenerate deliberately, at a known-good commit, with
//! `VELNOR_UPDATE_GOLDENS=1`; the default mode compares and never writes.

use std::fs;
use std::path::{Path, PathBuf};

use velnor_actions_orchestrator::{prepare, render_staged_tree};

use crate::impl_common::{
    TestResult, fixture_manifest_json, git, install_fixture_release_manifest,
};
use crate::impl_schema2_routing::required_file;

/// Golden refresh switch; comparison is the default.
const UPDATE_ENV: &str = "VELNOR_UPDATE_GOLDENS";
const CI: &str = ".github/workflows/ci.yml";

fn fixture(case: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(case)
}

fn copy_dir(src: &Path, dst: &Path) -> TestResult {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let target = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&target)?;
            copy_dir(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Replace the synthetic release identity with stable placeholders.
fn normalize(text: &str) -> Result<String, Box<dyn std::error::Error>> {
    let manifest: serde_json::Value = serde_json::from_str(&fixture_manifest_json())?;
    let field = |name: &str| {
        manifest
            .get(name)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| format!("manifest lacks {name}"))
    };
    let mut out = text.to_owned();
    let targets = manifest
        .get("targets")
        .and_then(serde_json::Value::as_array)
        .ok_or("manifest lacks targets")?;
    for target in targets {
        let name = target
            .get("target")
            .and_then(serde_json::Value::as_str)
            .ok_or("target name")?;
        let digest = target
            .get("sha256")
            .and_then(serde_json::Value::as_str)
            .ok_or("target digest")?;
        out = out.replace(digest, &format!("<sha256:{name}>"));
    }
    out = out.replace(&field("commit")?, "<commit>");
    Ok(out.replace(&field("version")?, "<version>"))
}

/// Generated `ci.yml` of one fixture, normalized.
pub(crate) fn generate(case: &str) -> Result<String, Box<dyn std::error::Error>> {
    let dir = tempfile::TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "main"], root)?;
    copy_dir(&fixture(case).join("input"), root)?;
    install_fixture_release_manifest(root)?;
    git(&["add", "."], root)?;
    let tree = render_staged_tree(&prepare(root)?)?;
    normalize(required_file(&tree, CI)?)
}

/// Compare against the golden, or refresh it under [`UPDATE_ENV`].
pub(crate) fn check_golden(case: &str) -> TestResult {
    let actual = generate(case)?;
    let golden = fixture(case).join("expected").join(CI);
    if std::env::var(UPDATE_ENV).is_ok_and(|value| value == "1") {
        fs::create_dir_all(golden.parent().ok_or("golden parent")?)?;
        fs::write(&golden, &actual)?;
        return Ok(());
    }
    let expected = fs::read_to_string(&golden)
        .map_err(|err| format!("missing golden {case}: {err}; regenerate with {UPDATE_ENV}=1"))?;
    if expected == actual {
        return Ok(());
    }
    let line = expected
        .lines()
        .zip(actual.lines())
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| expected.lines().count().min(actual.lines().count()))
        + 1;
    Err(format!(
        "golden mismatch {case}/{CI} at line {line}: expected {:?}, actual {:?}",
        expected.lines().nth(line - 1),
        actual.lines().nth(line - 1)
    )
    .into())
}

#[test]
fn bun_workspace_without_suites_is_byte_identical() -> TestResult {
    check_golden("bun-workspace")
}

#[test]
fn bun_workspace_with_suites_matches_its_golden() -> TestResult {
    check_golden("bun-workspace-suites")
}

#[test]
fn bun_golden_generation_is_deterministic() -> TestResult {
    assert_eq!(generate("bun-workspace")?, generate("bun-workspace")?);
    Ok(())
}
