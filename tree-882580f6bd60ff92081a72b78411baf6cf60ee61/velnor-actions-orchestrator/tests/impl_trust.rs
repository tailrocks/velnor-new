//! Trust cases: rev validation, source-build identity, digests, hints.

use std::path::Path;

use tempfile::TempDir;
use velnor_actions_contract::{FinalStatus, ObligationDecision, Plan};
use velnor_actions_orchestrator::{merge_internal, plan_internal};

use crate::impl_common::{
    TestResult, config_with_branch, fixture_manifest_json, git, git_line, make_repo,
    passing_reports, plan_for_source_change,
};

/// Two-commit single-package repo plus base and head SHAs.
fn two_commit_repo() -> Result<(TempDir, String, String), Box<dyn std::error::Error>> {
    let repo = make_repo(config_with_branch())?;
    let root = repo.path();
    commit(root, "one")?;
    std::fs::write(root.join("src/lib.rs"), "pub fn f() {}\npub fn g() {}\n")?;
    let head = commit(root, "two")?;
    let base = git_line(&["rev-parse", "HEAD~1"], root)?;
    Ok((repo, base, head))
}

/// Two-member workspace fixture, uncommitted; tests commit as needed.
fn two_pkg_repo() -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = TempDir::new()?;
    let root = dir.path();
    git(&["init", "-b", "testmain"], root)?;
    git(&["config", "user.email", "test@example.com"], root)?;
    git(&["config", "user.name", "Test"], root)?;
    git(&["config", "commit.gpgsign", "false"], root)?;
    std::fs::create_dir_all(root.join(".velnor"))?;
    std::fs::write(root.join(".velnor/config.toml"), config_with_branch())?;
    std::fs::write(
        root.join(".velnor/release-manifest.json"),
        fixture_manifest_json(),
    )?;
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"alpha\", \"beta\"]\n",
    )?;
    for member in ["alpha", "beta"] {
        let member_dir = root.join(member);
        std::fs::create_dir_all(member_dir.join("src"))?;
        let manifest =
            format!("[package]\nname = \"{member}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n");
        std::fs::write(member_dir.join("Cargo.toml"), manifest)?;
        std::fs::write(member_dir.join("src/lib.rs"), "pub fn f() {}\n")?;
    }
    Ok(dir)
}

/// Commit everything; returns the new HEAD SHA.
fn commit(root: &Path, message: &str) -> Result<String, Box<dyn std::error::Error>> {
    git(&["add", "."], root)?;
    git(&["commit", "-m", message], root)?;
    git_line(&["rev-parse", "HEAD"], root)
}

/// Plan as a pull request with an optional manifest; plan plus warnings.
fn plan_with(
    root: &Path,
    base: Option<&str>,
    head: &str,
    manifest: Option<serde_json::Value>,
) -> Result<(Plan, Vec<String>), Box<dyn std::error::Error>> {
    let mut request = serde_json::json!({
        "schema": 1, "run_key": "local", "base": base, "head": head,
        "event": "pull_request", "root": root.display().to_string(),
    });
    if let Some(manifest) = manifest {
        request["baseline_manifest"] = manifest;
    }
    let response = plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    let warnings: Vec<String> = serde_json::from_value(value["plan"]["warnings"].clone())?;
    Ok((plan, warnings))
}

/// Baseline manifest JSON binding every plan obligation exactly.
fn manifest_for(plan: &Plan, base: &str) -> serde_json::Value {
    let compat = velnor_actions_contract::digest_b3(b"compat");
    let tasks: Vec<serde_json::Value> = plan
        .obligations
        .iter()
        .map(|ob| {
            serde_json::json!({
                "task_id": ob.task_id, "task_digest": ob.task_digest,
                "input_digest": ob.input_digest, "proof_run_id": 7, "observed_run_id": 7,
            })
        })
        .collect();
    serde_json::json!({
        "schema": 1, "repository_id": velnor_actions_contract::digest_b3(b"repo"),
        "source_commit": base, "ref": "refs/heads/testmain", "event": "push",
        "workflow_ref": "o/r/.github/workflows/velnor.yml@refs/heads/testmain",
        "run_id": 7, "run_attempt": 1, "final_status": "passed",
        "generator_version": plan.generator.version, "generator_sha256": plan.generator.sha256,
        "compatibility_id": compat, "artifact_id": 9,
        "artifact_name": format!("velnor-baseline-{base}-{compat}"), "tasks": tasks,
    })
}

#[test]
fn injection_revs_broaden_without_spawning_flags() -> TestResult {
    let repo = two_pkg_repo()?;
    let root = repo.path();
    let base = commit(root, "one")?;
    std::fs::write(
        root.join("beta/src/lib.rs"),
        "pub fn f() {}\npub fn g() {}\n",
    )?;
    let head = commit(root, "two")?;
    for evil in [
        "--output=/tmp/velnor_trust_pwn",
        "$(touch /tmp/velnor_trust_pwn)",
        "-x",
    ] {
        let (plan, warnings) = plan_with(root, Some(evil), &head, None)?;
        assert!(
            plan.task_ids.iter().any(|id| id.contains("alpha")),
            "broadens: {:?}",
            plan.task_ids
        );
        assert!(
            plan.task_ids.iter().any(|id| id.contains("beta")),
            "broadens: {:?}",
            plan.task_ids
        );
        assert!(
            warnings.iter().any(|w| w.contains("bad_base")),
            "{warnings:?}"
        );
        let err = plan_with(root, Some(&base), evil, None).expect_err("evil head rejected");
        assert!(err.to_string().contains("bad_head"), "got {err}");
        assert!(
            !std::path::Path::new("/tmp/velnor_trust_pwn").exists(),
            "no flag effect"
        );
    }
    Ok(())
}

#[test]
fn short_sha_base_still_classifies() -> TestResult {
    let repo = two_pkg_repo()?;
    let root = repo.path();
    let base = commit(root, "one")?;
    std::fs::write(
        root.join("beta/src/lib.rs"),
        "pub fn f() {}\npub fn g() {}\n",
    )?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_with(root, Some(&base[..7]), &head, None)?;
    for member in ["alpha", "beta"] {
        let obs: Vec<_> = plan
            .obligations
            .iter()
            .filter(|ob| ob.task_id.contains(member))
            .collect();
        let want = if member == "beta" {
            "affected_by_change"
        } else {
            "unproven"
        };
        assert!(
            !obs.is_empty() && obs.iter().all(|ob| ob.reason == want),
            "{member}: {obs:?}"
        );
    }
    assert!(warnings.is_empty(), "no warnings: {warnings:?}");
    Ok(())
}

#[test]
fn new_package_at_head_marks_affected() -> TestResult {
    let repo = two_pkg_repo()?;
    let root = repo.path();
    std::fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"alpha\", \"beta\", \"gamma\"]\n",
    )?;
    let base = commit(root, "one")?;
    let gamma = root.join("gamma");
    std::fs::create_dir_all(gamma.join("src"))?;
    std::fs::write(
        gamma.join("Cargo.toml"),
        "[package]\nname = \"gamma\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    std::fs::write(gamma.join("src/lib.rs"), "pub fn f() {}\n")?;
    std::fs::write(
        root.join("beta/src/lib.rs"),
        "pub fn f() {}\npub fn g() {}\n",
    )?;
    let head = commit(root, "two")?;
    let (plan, warnings) = plan_with(root, Some(&base), &head, None)?;
    for member in ["alpha", "beta", "gamma"] {
        let obs: Vec<_> = plan
            .obligations
            .iter()
            .filter(|ob| ob.task_id.contains(member))
            .collect();
        let want = if member == "alpha" {
            "unproven"
        } else {
            "affected_by_change"
        };
        assert!(
            !obs.is_empty() && obs.iter().all(|ob| ob.reason == want),
            "{member}: {obs:?}"
        );
    }
    assert!(
        !warnings
            .iter()
            .any(|w| w.contains("comparison_unavailable")),
        "added skipped: {warnings:?}"
    );
    Ok(())
}

#[test]
fn source_build_skips_lookup_with_reason() -> TestResult {
    let (repo, base, head) = two_commit_repo()?;
    // P03: no zero digests. Inject the explicit unresolved marker (the
    // default when the exe digest is unreadable); the native `b3-`
    // default instead attempts live lookup and lets provenance decide.
    let request = serde_json::json!({
        "schema": 1, "run_key": "local", "base": Some(&base), "head": head,
        "event": "pull_request", "root": repo.path().display().to_string(),
        "generator": {
            "version": env!("CARGO_PKG_VERSION"),
            "target": "x86_64-unknown-linux-gnu",
            "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        },
    });
    let response = plan_internal(&request.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&response)?;
    let plan: Plan = serde_json::from_value(value["plan"].clone())?;
    assert_ne!(plan.generator.sha256, "0".repeat(64));
    assert_eq!(
        plan.baseline.reason.as_deref(),
        Some("generator_unverifiable_source_build")
    );
    Ok(())
}

#[test]
fn release_lock_resolves_generator_identity() -> TestResult {
    let (repo, base, head) = two_commit_repo()?;
    let sha = "e".repeat(64);
    let version = env!("CARGO_PKG_VERSION");
    let mut bins = String::new();
    for target in velnor_actions_contract::SUPPORTED_TARGETS {
        use std::fmt::Write as _;
        write!(bins, "[[generator.binaries]]\ntarget = \"{target}\"\nartifact = \"https://example.invalid/r/{version}/{target}\"\nsha256 = \"{sha}\"\n").expect("write to String");
    }
    let lock = format!(
        "schema = 1\n[generator]\nbinary = \"velnor-actions\"\nversion = \"{version}\"\n{bins}[mise-bootstrap]\nversion = \"2026.9.16\"\nartifact = \"https://example.invalid/mise\"\nsha256 = \"{}\"\n[[actions]]\nname = \"actions/checkout\"\nversion = \"v7.0.1\"\nsha = \"{}\"\nreviewed = \"2026-09-28\"\n",
        "c".repeat(64),
        "d".repeat(40)
    );
    std::fs::write(repo.path().join(".velnor/generator.lock"), lock)?;
    let (plan, _) = plan_with(repo.path(), Some(&base), &head, None)?;
    assert_eq!(plan.generator.sha256, sha);
    assert_ne!(
        plan.baseline.reason.as_deref(),
        Some("generator_unverifiable_source_build")
    );
    Ok(())
}

#[test]
fn merge_flags_wrong_manifest_digest() -> TestResult {
    // Serialization cannot fail for this struct, so the wrong-digest branch
    // pins the enforced comparison; the error branch fails closed the same
    // way without ever digesting an empty default.
    let (_repo, mut plan) = plan_for_source_change()?;
    let base = plan.base.clone().expect("base");
    let mut manifest = manifest_for(&plan, &base);
    // Plan validation rejects baseline artifact names (see the revert test),
    // so both sides share a plan-kind name; merge compares equality only.
    manifest["artifact_name"] = serde_json::json!("velnor-plan-local");
    let first = 0;
    plan.obligations[first].decision = ObligationDecision::CoveredByTrustedBaseline;
    plan.obligations[first].reason = "covered_by_trusted_baseline".to_owned();
    plan.obligations[first].baseline_proof = Some(velnor_actions_contract::BaselineProof {
        source_commit: base,
        run_id: 7,
        artifact_id: 9,
        artifact_name: "velnor-plan-local".to_owned(),
        manifest_digest: velnor_actions_contract::digest_b3(b"forged-manifest"),
    });
    plan.validate()?;
    let reports = passing_reports(&plan)?;
    let request = serde_json::json!({
        "schema": 1, "run_key": "local", "plan": plan, "matrix": plan.matrix,
        "matrix_reports": reports, "baseline_manifest": manifest,
        "required_job_ids": ["velnor-plan"],
        "required_jobs": [{"job_id": "velnor-plan", "conclusion": "success"}],
    });
    let final_report: velnor_actions_contract::FinalReport =
        serde_json::from_str(&merge_internal(&request.to_string())?)?;
    assert_eq!(final_report.status, FinalStatus::PlanningFailed);
    Ok(())
}

#[test]
fn coverage_warn_notes_no_publish_attempt() -> TestResult {
    let (repo, base, head) = two_commit_repo()?;
    let (seed, _) = plan_with(repo.path(), Some(&base), &head, None)?;
    let (_, warnings) = plan_with(
        repo.path(),
        Some(&base),
        &head,
        Some(manifest_for(&seed, &base)),
    )?;
    let noted = warnings
        .iter()
        .any(|w| w.contains("baseline_publish:forbidden") && w.contains("no_publish_attempted"));
    assert!(noted, "{warnings:?}");
    Ok(())
}
