use std::path::Path;

use velnor_actions_contract::digest_b3;
use velnor_actions_orchestrator_cover_baseline::cover_baseline::{
    baseline_download_args, baseline_entry_for, unix_now,
};
use velnor_actions_orchestrator_cover_baseline::cover_identity::{
    SOURCE_BUILD_REASON, is_source_build,
};
use velnor_actions_orchestrator_cover_compat::cover_compat::baseline_artifact_numeric_id;

fn base() -> String {
    "a".repeat(40)
}

fn entry_name(base: &str) -> String {
    format!("velnor-baseline-{base}-{}", digest_b3(b"c"))
}

fn manifest_json(base: &str, name: &str) -> serde_json::Value {
    let digest = digest_b3(b"d");
    let numeric = baseline_artifact_numeric_id(name);
    serde_json::json!({
        "schema": 2,
        "repository_id": digest,
        "source_commit": base,
        "ref": "refs/heads/testmain",
        "event": "push",
        "workflow_ref": "o/r/.github/workflows/ci.yml@refs/heads/testmain",
        "run_id": 7,
        "run_attempt": 1,
        "final_status": "passed",
        "generator_version": "0.1.0",
        "generator_sha256": "1".repeat(64),
        "compatibility_id": digest,
        "artifact_id": numeric,
        "artifact_name": name,
        "tasks": [],
    })
}

fn staged_entry(name: &str, payload: &str) -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("tempdir");
    let entry = tmp.path().join(name);
    std::fs::create_dir(&entry).expect("entry");
    std::fs::write(entry.join("baseline.json"), payload).expect("manifest");
    tmp
}

#[test]
fn entry_valid_single_payload_loads() {
    let base = base();
    let name = entry_name(&base);
    let numeric = baseline_artifact_numeric_id(&name);
    let manifest = manifest_json(&base, &name);
    let tmp = staged_entry(&name, &manifest.to_string());
    let found = baseline_entry_for(&tmp.path().join(&name), &base, 7, 1, numeric).expect("loads");
    assert_eq!(found.source_commit, base);
    assert_eq!(found.run_id, 7);
    assert_eq!(found.artifact_name, name);
}

#[test]
fn entry_wrong_dir_name_misses() {
    let base = base();
    let name = entry_name(&base);
    let numeric = baseline_artifact_numeric_id(&name);
    let manifest = manifest_json(&base, &name);
    let tmp = staged_entry("velnor-baseline-wrong", &manifest.to_string());
    assert!(
        baseline_entry_for(
            &tmp.path().join("velnor-baseline-wrong"),
            &base,
            7,
            1,
            numeric
        )
        .is_none()
    );
}

#[test]
fn entry_extra_sibling_file_misses() {
    let base = base();
    let name = entry_name(&base);
    let numeric = baseline_artifact_numeric_id(&name);
    let manifest = manifest_json(&base, &name);
    let tmp = staged_entry(&name, &manifest.to_string());
    std::fs::write(tmp.path().join(&name).join("extra.json"), "{}").expect("sibling");
    assert!(
        baseline_entry_for(&tmp.path().join(&name), &base, 7, 1, numeric).is_none(),
        "entry dir must carry exactly baseline.json"
    );
}

#[test]
fn entry_attempt_mismatch_misses() {
    let base = base();
    let name = entry_name(&base);
    let numeric = baseline_artifact_numeric_id(&name);
    let manifest = manifest_json(&base, &name);
    let tmp = staged_entry(&name, &manifest.to_string());
    assert!(
        baseline_entry_for(&tmp.path().join(&name), &base, 7, 3, numeric).is_none(),
        "claims attempt 1, success on 3"
    );
}

#[test]
fn download_args_exact_artifact_pins_repo() {
    let base = base();
    let name = entry_name(&base);
    let args = baseline_download_args(
        &base,
        ".github/workflows/ci.yml",
        "testmain",
        Some(&name),
        7,
        Path::new("/tmp/x"),
        "o/r",
    );
    let argv: Vec<String> = args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert_eq!(&argv[0..4], &["run", "download", "7", "--name"]);
    assert_eq!(argv[4], name);
    assert!(argv.windows(2).any(|pair| pair == ["--repo", "o/r"]));
}

#[test]
fn download_args_without_exact_artifact_is_empty() {
    let base = base();
    assert!(
        baseline_download_args(
            &base,
            ".github/workflows/ci.yml",
            "testmain",
            None,
            7,
            Path::new("/tmp/x"),
            "o/r",
        )
        .is_empty()
    );
    assert!(
        baseline_download_args(
            &base,
            ".github/workflows/ci.yml",
            "testmain",
            Some(""),
            7,
            Path::new("/tmp/x"),
            "o/r",
        )
        .is_empty()
    );
    let name = entry_name(&base);
    assert!(
        baseline_download_args(
            &base,
            ".github/workflows/ci.yml",
            "testmain",
            Some(&name),
            7,
            Path::new("/tmp/x"),
            "not a repo",
        )
        .is_empty(),
        "malformed repo yields no command"
    );
}

#[test]
fn download_args_malformed_base_yields_no_command() {
    let name = entry_name(&base());
    assert!(
        baseline_download_args(
            "xyz",
            ".github/workflows/ci.yml",
            "testmain",
            Some(&name),
            7,
            Path::new("/tmp/x"),
            "o/r",
        )
        .is_empty(),
        "short SHAs never name a lookup"
    );
}

#[test]
fn entry_without_payload_misses() {
    let base = base();
    let name = entry_name(&base);
    let numeric = baseline_artifact_numeric_id(&name);
    let tmp = tempfile::tempdir().expect("tempdir");
    let entry = tmp.path().join(&name);
    std::fs::create_dir(&entry).expect("entry");
    assert!(
        baseline_entry_for(&entry, &base, 7, 1, numeric).is_none(),
        "empty entry dir carries no evidence"
    );
}

#[test]
fn entry_malformed_json_misses() {
    let base = base();
    let name = entry_name(&base);
    let numeric = baseline_artifact_numeric_id(&name);
    let tmp = staged_entry(&name, "{oops");
    assert!(
        baseline_entry_for(&tmp.path().join(&name), &base, 7, 1, numeric).is_none(),
        "unparseable payload never loads"
    );
}

#[test]
fn unix_now_is_plausible() {
    let now = unix_now();
    assert!(now >= 1_700_000_000, "unexpected: {now}");
    assert_ne!(now, u64::MAX, "clock failure fails closed elsewhere");
}

#[test]
fn source_build_gate_pins() {
    assert!(is_source_build(""));
    assert!(is_source_build(&"0".repeat(64)));
    assert!(!is_source_build(&"1".repeat(64)));
    assert_eq!(SOURCE_BUILD_REASON, "generator_unverifiable_source_build");
}
