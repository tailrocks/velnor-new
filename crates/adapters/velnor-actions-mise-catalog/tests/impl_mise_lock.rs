//! Bootstrap lock parse and lock-vs-manifest verify cases.
use velnor_actions_contract::SUPPORTED_TARGETS;
use velnor_actions_mise_catalog::catalog::lock::{
    LockError, parse_generator_lock, parse_release_manifest, verify_lock_against_manifest,
};

const GENERATOR_VERSION: &str = env!("CARGO_PKG_VERSION");

fn binary_record(target: &str, sha: &str) -> String {
    format!(
        "[[generator.binaries]]\ntarget = \"{target}\"\nartifact = \"https://github.com/tailrocks/velnor-new/releases/download/v{GENERATOR_VERSION}/velnor-actions-{GENERATOR_VERSION}-{target}\"\nsha256 = \"{sha}\"\n"
    )
}

fn lock_text(sha: &str) -> String {
    let bins = SUPPORTED_TARGETS
        .iter()
        .map(|target| binary_record(target, sha))
        .collect::<String>();
    format!(
        "schema = 1\n[generator]\nbinary = \"velnor-actions\"\nversion = \"{GENERATOR_VERSION}\"\ncommit = \"{}\"\n{bins}[mise-bootstrap]\nversion = \"2026.9.18\"\nartifact = \"https://example.invalid/mise\"\nsha256 = \"{}\"\n[[actions]]\nname = \"actions/checkout\"\nversion = \"v7.0.1\"\nsha = \"{}\"\nreviewed = \"2026-09-28\"\n",
        "a".repeat(40),
        "c".repeat(64),
        "d".repeat(40)
    )
}

fn manifest_text(sha: &str) -> String {
    let targets = SUPPORTED_TARGETS
        .iter()
        .map(|target| format!("{{\"target\":\"{target}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v{GENERATOR_VERSION}/velnor-actions-{GENERATOR_VERSION}-{target}\",\"sha256\":\"{sha}\"}}"))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"{GENERATOR_VERSION}\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{}\",\"targets\":[{targets}]}}",
        "a".repeat(40)
    )
}

#[test]
fn lock_matches_manifest_per_target() -> Result<(), LockError> {
    let lock = parse_generator_lock(&lock_text(&"a".repeat(64)))?;
    let manifest = parse_release_manifest(&manifest_text(&"a".repeat(64)))?;
    verify_lock_against_manifest(&lock, &manifest)
}

#[test]
fn lock_mismatch_mutable_and_missing_target_fail() -> Result<(), LockError> {
    let lock = parse_generator_lock(&lock_text(&"a".repeat(64)))?;
    let drifted = parse_release_manifest(&manifest_text(&"b".repeat(64)))?;
    assert!(verify_lock_against_manifest(&lock, &drifted).is_err());
    let mutable = lock_text(&"a".repeat(64)).replace("https://example.invalid", "https://x/latest");
    assert!(parse_generator_lock(&mutable).is_err());
    let dropped = lock_text(&"a".repeat(64))
        .replace(&binary_record("aarch64-apple-darwin", &"a".repeat(64)), "");
    assert!(parse_generator_lock(&dropped).is_err());
    assert!(parse_generator_lock("schema = 1\n[bogus\n").is_err());
    Ok(())
}

#[test]
fn lock_commit_diverged_from_manifest_fails() -> Result<(), LockError> {
    let lock = parse_generator_lock(&lock_text(&"a".repeat(64)))?;
    let manifest = parse_release_manifest(&manifest_text(&"a".repeat(64)))?;
    assert_eq!(lock.generator.commit, manifest.commit);
    let relabeled = manifest_text(&"a".repeat(64)).replace(
        &format!("\"commit\":\"{}\"", "a".repeat(40)),
        &format!("\"commit\":\"{}\"", "b".repeat(40)),
    );
    let drifted = parse_release_manifest(&relabeled)?;
    let err = verify_lock_against_manifest(&lock, &drifted).expect_err("commit must match");
    assert!(err.to_string().contains("commit:"), "{err}");
    Ok(())
}

#[test]
fn lock_commit_required_and_manifest_strict() -> Result<(), LockError> {
    let full = lock_text(&"a".repeat(64));
    let commit = "a".repeat(40);
    let segment = format!("commit = \"{commit}\"\n");
    assert!(full.contains(&segment), "fixture must carry commit");
    let missing = full.replace(&segment, "");
    let err = parse_generator_lock(&missing).expect_err("commit required");
    assert!(err.to_string().contains("missing_key:commit"), "{err}");
    for bad in ["xyz", &"A".repeat(40), &"c".repeat(39), ""] {
        let malformed = full.replace(&segment, &format!("commit = \"{bad}\"\n"));
        let err = parse_generator_lock(&malformed).expect_err("malformed commit");
        assert!(err.to_string().contains("malformed_commit"), "{err}");
    }
    let lock = parse_generator_lock(&full)?;
    assert_eq!(lock.generator.commit, commit);
    Ok(())
}
