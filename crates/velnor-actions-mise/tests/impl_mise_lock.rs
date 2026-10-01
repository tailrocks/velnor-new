//! Bootstrap lock parse and lock-vs-manifest verify cases.
use velnor_actions_mise::catalog::lock::{
    LockError, parse_generator_lock, parse_release_manifest, verify_lock_against_manifest,
};

fn binary_record(target: &str, sha: &str) -> String {
    format!(
        "[[generator.binaries]]\ntarget = \"{target}\"\nartifact = \"https://github.com/tailrocks/velnor-new/releases/download/v0.1.0/velnor-actions-0.1.0-{target}\"\nsha256 = \"{sha}\"\n"
    )
}

fn lock_text(sha: &str) -> String {
    let bins = binary_record("x86_64-unknown-linux-gnu", sha)
        + &binary_record("aarch64-apple-darwin", sha)
        + &binary_record("x86_64-apple-darwin", sha);
    format!(
        "schema = 1\n[generator]\nbinary = \"velnor-actions\"\nversion = \"0.1.0\"\n{bins}[mise-bootstrap]\nversion = \"2026.9.18\"\nartifact = \"https://example.invalid/mise\"\nsha256 = \"{}\"\n[[actions]]\nname = \"actions/checkout\"\nversion = \"v7.0.1\"\nsha = \"{}\"\nreviewed = \"2026-09-28\"\n",
        "c".repeat(64),
        "d".repeat(40)
    )
}

fn manifest_text(sha: &str) -> String {
    let targets = ["x86_64-unknown-linux-gnu", "aarch64-apple-darwin", "x86_64-apple-darwin"]
        .iter()
        .map(|t| format!("{{\"target\":\"{t}\",\"artifact\":\"https://github.com/tailrocks/velnor-new/releases/download/v0.1.0/velnor-actions-0.1.0-{t}\",\"sha256\":\"{sha}\"}}"))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"schema\":1,\"version\":\"0.1.0\",\"repository\":\"tailrocks/velnor-new\",\"commit\":\"{}\",\"targets\":[{targets}]}}",
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
    let dropped = lock_text(&"a".repeat(64)).replace("[[generator.binaries]]\ntarget = \"x86_64-apple-darwin\"\nartifact = \"https://github.com/tailrocks/velnor-new/releases/download/v0.1.0/velnor-actions-0.1.0-x86_64-apple-darwin\"\nsha256 = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\n", "");
    assert!(parse_generator_lock(&dropped).is_err());
    assert!(parse_generator_lock("schema = 1\n[bogus\n").is_err());
    Ok(())
}
