use std::error::Error;

use serde_json::json;

use super::{artifact_record, normalize_workflow_path, run_record};

#[test]
fn workflow_api_path_can_be_plain_or_ref_suffixed() {
    for value in [
        ".github/workflows/ci.yml",
        ".github/workflows/ci.yml@main",
        ".github/workflows/ci.yml@refs/heads/main",
    ] {
        assert_eq!(
            normalize_workflow_path(value, "main").as_deref(),
            Ok(".github/workflows/ci.yml@main")
        );
    }
}

#[test]
fn rejects_wrong_workflow_path_and_wrong_ref() {
    assert!(normalize_workflow_path(".github/workflows/release.yml@main", "main").is_err());
    assert!(normalize_workflow_path(".github/workflows/ci.yml@feature", "main").is_err());
}

#[test]
fn run_attempt_parser_binds_run_attempt_source_and_default_branch() -> Result<(), Box<dyn Error>> {
    let value = json!({
        "id": 42,
        "run_attempt": 2,
        "head_branch": "main",
        "head_sha": "0123456789abcdef0123456789abcdef01234567",
        "repository": {"full_name": "tailrocks/velnor-new"},
        "path": ".github/workflows/ci.yml",
        "event": "workflow_dispatch",
        "status": "completed",
        "conclusion": "success"
    });
    let record = run_record(&value, 42, 2, "main")?;

    assert_eq!(record.repository, "tailrocks/velnor-new");
    assert_eq!(record.path_ref, ".github/workflows/ci.yml@main");
    assert!(run_record(&value, 42, 1, "main").is_err());
    assert!(run_record(&value, 42, 2, "release").is_err());
    Ok(())
}

#[test]
fn artifact_parser_rejects_expired_or_wrong_run_artifacts() -> Result<(), Box<dyn Error>> {
    let value = json!({
        "id": 7,
        "name": "velnor-qualification-cache-receipt-v1",
        "digest": format!("sha256:{}", "a".repeat(64)),
        "size_in_bytes": 128,
        "expired": false,
        "workflow_run": {"id": 42, "head_branch": "main", "head_sha": "0123456789abcdef0123456789abcdef01234567"}
    });
    let record = artifact_record(&value, 42)?;
    assert_eq!(record.id, 7);
    assert!(artifact_record(&value, 43).is_err());

    let mut expired = value.clone();
    expired["expired"] = json!(true);
    assert!(artifact_record(&expired, 42).is_err());
    Ok(())
}
