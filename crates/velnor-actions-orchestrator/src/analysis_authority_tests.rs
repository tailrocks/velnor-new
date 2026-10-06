use super::*;

fn listing() -> serde_json::Value {
    serde_json::json!({"artifacts":[{
        "id":7,"name":"analysis","expired":false,"size_in_bytes":128,
        "digest":format!("sha256:{}", "b".repeat(64)),
        "workflow_run":{"id":9,"head_sha":"a".repeat(40),"head_branch":"main"}
    }]})
}

fn lookup_fixture() -> BaselineLookup {
    BaselineLookup::new(
        &"a".repeat(40),
        ".github/workflows/ci.yml",
        "main",
        "owner/repo",
    )
    .expect("lookup")
}

#[test]
fn artifact_requires_unique_fresh_exact_run_digest() {
    let lookup = lookup_fixture();
    let run = SelectedBaseRun {
        run_id: 9,
        attempt: 2,
    };
    assert!(select_artifact(&listing().to_string(), "analysis", &lookup, run).is_ok());
    for mutation in [
        "expired",
        "digest",
        "run",
        "head",
        "branch",
        "size",
        "duplicate",
    ] {
        let mut value = listing();
        match mutation {
            "expired" => value["artifacts"][0]["expired"] = serde_json::Value::Null,
            "digest" => value["artifacts"][0]["digest"] = serde_json::Value::Null,
            "run" => value["artifacts"][0]["workflow_run"]["id"] = 8.into(),
            "head" => value["artifacts"][0]["workflow_run"]["head_sha"] = "b".repeat(40).into(),
            "branch" => value["artifacts"][0]["workflow_run"]["head_branch"] = "topic".into(),
            "size" => value["artifacts"][0]["size_in_bytes"] = (9 * 1024 * 1024).into(),
            "duplicate" => {
                let entry = value["artifacts"][0].clone();
                value["artifacts"]
                    .as_array_mut()
                    .expect("array")
                    .push(entry);
            }
            _ => unreachable!("fixture case"),
        }
        assert!(
            select_artifact(&value.to_string(), "analysis", &lookup, run).is_err(),
            "{mutation}"
        );
    }
}

fn zip_payload(names: &[&str]) -> Vec<u8> {
    use std::io::Write as _;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for name in names {
        writer
            .start_file(
                *name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored)
                    .unix_permissions(0o644),
            )
            .expect("entry");
        writer.write_all(b"{}").expect("payload");
    }
    writer.finish().expect("archive").into_inner()
}

#[test]
fn archive_accepts_only_one_named_bounded_regular_file() {
    assert_eq!(
        archive::payload(&zip_payload(&["analysis.json"])),
        Ok("{}".into())
    );
    for names in [
        &["../analysis.json"][..],
        &["analysis.json", "extra.json"][..],
        &["other.json"][..],
    ] {
        assert!(archive::payload(&zip_payload(names)).is_err());
    }
    assert!(archive::payload(b"not a ZIP").is_err());
}

#[test]
fn artifact_compatibility_binds_helper_and_catalog() {
    let base = "a".repeat(40);
    let cargo = ToolCatalog::pinned().rustup_toolchain().to_owned();
    let first = analysis_artifact_name(&base, &"b".repeat(64), &cargo).expect("name");
    let second = analysis_artifact_name(&base, &"c".repeat(64), &cargo).expect("name");
    assert_ne!(first, second);
    assert!(analysis_artifact_name(&base, &"0".repeat(64), &cargo).is_err());
    assert!(analysis_artifact_name(&base, &"b".repeat(64), "wrong").is_err());
}

#[test]
fn run_requires_exact_source_attempt_workflow_and_repository() {
    let catalog = ToolCatalog::pinned();
    let lookup = lookup_fixture();
    let helper = "b".repeat(64);
    let inputs = AnalysisLookupInputs {
        catalog: &catalog,
        root: Path::new("."),
        base: &lookup.base_sha,
        workflow: &lookup.workflow,
        branch: &lookup.branch,
        repository: Some(&lookup.repo),
        helper_sha256: &helper,
    };
    let selected = SelectedBaseRun {
        run_id: 9,
        attempt: 2,
    };
    let original = serde_json::json!({
        "id":9,"run_attempt":2,"head_sha":lookup.base_sha,"head_branch":"main",
        "event":"push","status":"completed","conclusion":"success",
        "path":lookup.workflow,"repository":{"full_name":"owner/repo"},
        "head_repository":{"full_name":"owner/repo"},
    });
    assert!(verify_run_value(&original, inputs, &lookup, selected).is_ok());
    let mut case_variant = original.clone();
    case_variant["repository"]["full_name"] = "Owner/Repo".into();
    case_variant["head_repository"]["full_name"] = "OWNER/REPO".into();
    assert!(verify_run_value(&case_variant, inputs, &lookup, selected).is_ok());
    for (field, value) in [
        ("id", 8.into()),
        ("run_attempt", 1.into()),
        ("head_sha", "c".repeat(40).into()),
        ("head_branch", "topic".into()),
        ("event", "pull_request".into()),
        ("status", "in_progress".into()),
        ("conclusion", "failure".into()),
        ("path", ".github/workflows/other.yml".into()),
        ("repository", serde_json::json!({"full_name":"other/repo"})),
        (
            "head_repository",
            serde_json::json!({"full_name":"fork/repo"}),
        ),
    ] {
        let mut value_run = original.clone();
        value_run[field] = value;
        assert!(
            verify_run_value(&value_run, inputs, &lookup, selected).is_err(),
            "{field}"
        );
    }
}

#[test]
fn api_repository_identity_uses_canonical_slug() {
    assert!(repository_matches(Some("ChainArgos/Foo"), "chainargos/foo"));
    assert!(!repository_matches(
        Some("ChainArgos/Other"),
        "chainargos/foo"
    ));
    assert!(!repository_matches(
        Some("https://github.com/ChainArgos/Foo"),
        "chainargos/foo"
    ));
    assert!(!repository_matches(None, "chainargos/foo"));
}

#[test]
fn duplicate_json_keys_cannot_supply_artifact_authority() {
    let listed = listing()
        .to_string()
        .replace("\"expired\":false", "\"expired\":true,\"expired\":false");
    assert!(
        select_artifact(
            &listed,
            "analysis",
            &lookup_fixture(),
            SelectedBaseRun {
                run_id: 9,
                attempt: 2
            }
        )
        .is_err()
    );
}

#[test]
fn archive_rejects_symlink_directory_and_executable_entries() {
    let mut archive_bytes = zip_payload(&["analysis.json"]);
    let central = archive_bytes
        .windows(4)
        .position(|bytes| bytes == b"PK\x01\x02")
        .expect("central entry");
    for mode in [0o120777_u32, 0o040755, 0o100755] {
        archive_bytes[central + 38..central + 42].copy_from_slice(&(mode << 16).to_le_bytes());
        assert!(archive::payload(&archive_bytes).is_err(), "{mode:o}");
    }
}
