use super::*;
#[test]
fn admission_digest_binds_exact_bytes_and_rejects_missing_or_altered_outputs() {
    let bytes = b"authenticated immutable receipt handoff";
    let digest = super::super::super::sha256(bytes);
    assert!(check_digest(bytes, &digest).is_ok());
    for bad in [
        String::new(),
        "a".repeat(63),
        "a".repeat(64),
        digest.to_uppercase(),
    ] {
        assert!(check_digest(bytes, &bad).is_err());
    }
    assert!(check_digest(b"changed source or receipt", &digest).is_err());
}
#[test]
fn prepare_rejects_even_present_empty_credentials_before_source_reads() {
    assert!(credentials_absent(false).is_ok());
    let error = credentials_absent(true).expect_err("parent credential present");
    assert!(
        error
            .to_string()
            .contains("tap_prepare_credentials_present")
    );
}
#[test]
fn admission_metadata_read_is_bounded_and_refuses_symlinks() {
    let temp = tempfile::tempdir().expect("temp");
    let path = temp.path().join("admission");
    std::fs::write(&path, b"12345").expect("write");
    let budget = deadline().expect("deadline");
    assert_eq!(read(&path, 5, budget).expect("read"), b"12345");
    assert!(read(&path, 4, budget).is_err());
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(&path, &link).expect("link");
    assert!(read(&link, 5, budget).is_err());
}
#[test]
fn admission_exclusive_write_preserves_existing_foreign_bytes() {
    let temp = tempfile::tempdir().expect("temp");
    let path = temp.path().join(RECORD);
    std::fs::write(&path, b"existing").expect("write");
    assert!(
        crate::exclusive_write::write_exclusive(&path, b"replacement", "tap_admission").is_err()
    );
    assert_eq!(std::fs::read(&path).expect("preserved"), b"existing");
}

#[path = "tap_prepare_admission_fixtures.rs"]
mod fixtures;
#[test]
fn canonical_admission_binds_full_plan_receipt_policy_and_original_context() {
    let plan = fixtures::plan();
    let release_policy = fixtures::release_policy();
    let policy = fixtures::policy();
    policy.validate("fixture").expect("valid policy");
    let context = BTreeMap::from([
        ("GITHUB_RUN_ID".to_owned(), Some("42".to_owned())),
        (
            "VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT".to_owned(),
            Some("2".to_owned()),
        ),
    ]);
    let record = Admission {
        schema: SCHEMA.to_owned(),
        plan: plan.clone(),
        release_policy: release_policy.clone(),
        receipt: fixtures::receipt(),
        policy: policy.clone(),
        context: context.clone(),
    };
    let bytes = canonical_json_bytes(&record).expect("bytes");
    let digest = super::super::super::sha256(&bytes);
    assert!(parse_record(&bytes, &digest, &release_policy, &policy, &context).is_ok());
    let mut changed_plan = plan.clone();
    changed_plan.products[0].bins = vec!["other".to_owned()];
    let mut changed_record = serde_json::to_value(&record).expect("record");
    changed_record["plan"] = serde_json::to_value(&changed_plan).expect("plan");
    let changed_bytes = canonical_json_bytes(&changed_record).expect("encode changed plan");
    assert!(
        parse_record(
            &changed_bytes,
            &super::super::super::sha256(&changed_bytes),
            &release_policy,
            &policy,
            &context
        )
        .is_err()
    );
    let mut changed_policy = policy.clone();
    changed_policy
        .destinations
        .preview
        .as_mut()
        .expect("preview")
        .formula = "Formula/other.rb".to_owned();
    assert!(parse_record(&bytes, &digest, &release_policy, &changed_policy, &context).is_err());
    let mut changed_context = context.clone();
    changed_context.remove("VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT");
    assert!(parse_record(&bytes, &digest, &release_policy, &policy, &changed_context).is_err());
    changed_context.insert(
        "VELNOR_RELEASE_ORIGINAL_RUN_ATTEMPT".to_owned(),
        Some("3".to_owned()),
    );
    assert!(parse_record(&bytes, &digest, &release_policy, &policy, &changed_context).is_err());
    assert_missing_admission_fields(&record, &release_policy, &policy, &context);
    let mut altered = record;
    altered.receipt.source.commit_sha = "b".repeat(40);
    let bad = canonical_json_bytes(&altered).expect("changed receipt");
    assert!(parse_record(&bad, &digest, &release_policy, &policy, &context).is_err());
    assert!(
        parse_record(
            &bad,
            &super::super::super::sha256(&bad),
            &release_policy,
            &policy,
            &context
        )
        .is_err()
    );
}

fn assert_missing_admission_fields(
    record: &Admission,
    release_policy: &PackageReleasePolicy,
    policy: &HomebrewTapConfig,
    context: &BTreeMap<String, Option<String>>,
) {
    for field in [
        "schema",
        "plan",
        "release_policy",
        "receipt",
        "policy",
        "context",
    ] {
        let mut missing = serde_json::to_value(record).expect("value");
        missing.as_object_mut().expect("object").remove(field);
        let bad = canonical_json_bytes(&missing).expect("encode");
        assert!(
            parse_record(
                &bad,
                &super::super::super::sha256(&bad),
                release_policy,
                policy,
                context
            )
            .is_err()
        );
    }
}
