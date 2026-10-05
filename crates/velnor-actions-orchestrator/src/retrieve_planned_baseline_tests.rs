//! Exact selected parent survives concurrent same-base runs and reruns.

use super::*;
use crate::cover::revalidate::cover_revalidate_fixtures::{manifest_for, plan_for};
use std::cell::RefCell;
use velnor_actions_contract::{BaselineProof, PlanBaseline};

fn fixture() -> (Plan, BaselineManifest) {
    let base = "1".repeat(40);
    let mut parent = manifest_for(&base);
    parent.repository_id = digest_b3(b"github.com/o/r");
    let mut plan = plan_for(&parent, Some(&base));
    parent.compatibility_id =
        crate::cover_compat::baseline_compat_for_plan(&plan).expect("compatible plan");
    parent.artifact_name =
        crate::cover_baseline::lookup_artifact_name(&plan, &base).expect("artifact name");
    parent.artifact_id = crate::cover_compat::baseline_artifact_numeric_id(&parent.artifact_name);
    let digest = digest_b3(&canonical_json_bytes(&parent).expect("parent bytes"));
    plan.baseline = PlanBaseline::used(
        &base,
        parent.run_id,
        parent.artifact_id,
        &parent.artifact_name,
        &digest,
    )
    .expect("plan baseline");
    plan.obligations[0].baseline_proof = Some(
        BaselineProof::new(
            &base,
            parent.run_id,
            parent.artifact_id,
            &parent.artifact_name,
            &digest,
        )
        .expect("obligation proof"),
    );
    (plan, parent)
}

fn attempt_record(parent: &BaselineManifest) -> serde_json::Value {
    serde_json::json!({
        "id": parent.run_id, "run_attempt": parent.run_attempt,
        "head_sha": parent.source_commit, "event": "push", "status": "completed",
        "conclusion": "success", "head_branch": "testmain",
        "path": ".github/workflows/ci.yml", "repository": {"full_name": "o/r"}
    })
}

fn resolve_mock(
    plan: &Plan,
    parent: &BaselineManifest,
    metadata: &serde_json::Value,
) -> (Option<BaselineManifest>, Vec<Vec<String>>) {
    let calls = RefCell::new(Vec::new());
    let payload = canonical_json_bytes(parent).expect("canonical parent");
    let archive = crate::cover::shard_baseline::archive::test_archive("baseline.json", &payload);
    let listed_artifact = serde_json::json!({
        "id": 99,
        "name": parent.artifact_name,
        "expired": false,
        "size_in_bytes": archive.len(),
        "digest": format!("sha256:{}", crate::cover_identity::generator::sha256_hex(&archive)),
    });
    let found = resolve_planned(
        plan,
        "o/r",
        "testmain",
        |args| {
            let args: Vec<String> = args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            calls.borrow_mut().push(args.clone());
            if args[1].contains("/artifacts?name=") {
                Ok(serde_json::json!({"total_count":1,"artifacts": [listed_artifact]}).to_string())
            } else {
                Ok(metadata.to_string())
            }
        },
        |args| {
            let args: Vec<String> = args
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            calls.borrow_mut().push(args.clone());
            assert_eq!(args, ["api", "repos/o/r/actions/artifacts/99/zip"]);
            Ok(archive.clone())
        },
    );
    (found, calls.into_inner())
}

#[test]
fn selected_parent_survives_newer_same_base_run_and_attempt() {
    let (plan, parent) = fixture();
    // Latest same-base run/attempt may now be run 8 or attempt 2. The
    // resolver never lists latest runs: it authenticates run 7 attempt 1.
    let (found, calls) = resolve_mock(&plan, &parent, &attempt_record(&parent));
    assert!(found.is_some());
    assert!(calls[0][1].contains("repos/o/r/actions/runs/7/artifacts?name="));
    assert!(calls[0][1].contains("&per_page=100&page=1"));
    assert_eq!(calls[1], ["api", "repos/o/r/actions/artifacts/99/zip"]);
    assert_eq!(calls[2], ["api", "repos/o/r/actions/runs/7/attempts/1"]);
    assert!(
        !calls
            .iter()
            .any(|args| args.iter().any(|arg| arg == "list"))
    );
}

#[test]
fn wrong_service_attempt_or_provenance_never_authenticates() {
    let (plan, parent) = fixture();
    for (field, wrong) in [
        ("id", serde_json::json!(8)),
        ("run_attempt", serde_json::json!(2)),
        ("head_sha", serde_json::json!("2".repeat(40))),
        ("event", serde_json::json!("pull_request")),
        ("status", serde_json::json!("in_progress")),
        ("conclusion", serde_json::json!("failure")),
        ("head_branch", serde_json::json!("evil")),
        ("path", serde_json::json!(".github/workflows/evil.yml")),
        ("repository", serde_json::json!({"full_name": "evil/r"})),
    ] {
        let mut record = attempt_record(&parent);
        record[field] = wrong;
        assert!(
            resolve_mock(&plan, &parent, &record).0.is_none(),
            "reject {field}"
        );
    }
}

#[test]
fn replacement_manifest_digest_never_downloads_as_selected_parent() {
    let (plan, mut parent) = fixture();
    parent.run_attempt = 2;
    let (found, calls) = resolve_mock(&plan, &parent, &attempt_record(&parent));
    assert!(found.is_none());
    assert_eq!(
        calls.len(),
        2,
        "digest rejection before attempt authentication"
    );
}

#[test]
fn repository_service_casing_preserves_canonical_identity() {
    let (plan, mut parent) = fixture();
    let mut metadata = attempt_record(&parent);
    metadata["repository"]["full_name"] = serde_json::json!("O/R");
    assert!(resolve_mock(&plan, &parent, &metadata).0.is_some());
    parent.workflow_ref = "O/R/.github/workflows/ci.yml@refs/heads/testmain".to_owned();
    assert!(authentic_attempt(
        &metadata.to_string(),
        &parent,
        "o/r",
        "testmain",
        ".github/workflows/ci.yml"
    ));
}

#[test]
fn attempt_workflow_path_accepts_exact_bare_and_documented_ref_forms() {
    let (_, mut parent) = fixture();
    parent.ref_ = "refs/heads/main".to_owned();
    for (workflow, actual_path) in [
        (".github/workflows/ci.yml", ".github/workflows/ci.yml"),
        (
            ".github/workflows/build.yml",
            ".github/workflows/build.yml@main",
        ),
    ] {
        parent.workflow_ref = format!("o/r/{workflow}@refs/heads/main");
        let mut record = attempt_record(&parent);
        record["head_branch"] = serde_json::json!("main");
        record["path"] = serde_json::json!(actual_path);
        assert!(authentic_attempt(
            &record.to_string(),
            &parent,
            "o/r",
            "main",
            workflow,
        ));
    }
    parent.workflow_ref = "o/r/.github/workflows/build.yml@refs/heads/main".to_owned();
    let mut record = attempt_record(&parent);
    record["head_branch"] = serde_json::json!("main");
    for wrong_path in [
        ".github/workflows/build.yml@other",
        ".github/workflows/build.yml@main-extra",
        ".github/workflows/build.yml@refs/heads/other",
        ".github/workflows/build.yml.bak@main",
    ] {
        record["path"] = serde_json::json!(wrong_path);
        assert!(!authentic_attempt(
            &record.to_string(),
            &parent,
            "o/r",
            "main",
            ".github/workflows/build.yml",
        ));
    }
    record["path"] = serde_json::json!(".github/workflows/build.yml@main");
    record["head_branch"] = serde_json::json!("other");
    assert!(!authentic_attempt(
        &record.to_string(),
        &parent,
        "o/r",
        "main",
        ".github/workflows/build.yml",
    ));
}
