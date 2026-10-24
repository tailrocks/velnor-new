use super::*;

#[test]
fn pinned_historical_catalog_has_distinct_authenticated_a_and_d_records() {
    let plan = parse_package_release_plan(PLAN).expect("pinned canonical plan");
    assert_eq!(sha256(PLAN), PLAN_SHA256);
    let a = parse_record(A_RECORD, "A", &plan).expect("A record");
    let d = parse_record(D_RECORD, "D", &plan).expect("D record");
    assert_eq!(
        a.release.source_sha,
        "41bad147b5ba9db09be99b07248531ce1bd867be"
    );
    assert_eq!(
        d.release.source_sha,
        "5094139b2658e793d36306a34a7a135554c4f7eb"
    );
    assert!(a.recovery.is_some());
    assert!(d.recovery.is_none());
    assert!(admission(&a, &plan, Some(A_ADMISSION)).is_ok());
    assert!(admission(&d, &plan, None).is_ok());
    assert!(admission(&a, &plan, None).is_err());
    assert!(admission(&d, &plan, Some(A_ADMISSION)).is_err());
    assert_eq!(A_ADMISSION.len(), 34_639);
    assert_eq!(
        sha256(A_ADMISSION),
        "73eacadf794d40c16e39e6693d09741ce9146cefc867d135f4a1fee023daa972"
    );
    let projection = crate::release_policy_admission::project_public_historical_admission(
        A_ADMISSION,
        &a.recovery
            .as_ref()
            .expect("A has recovery audit metadata")
            .source_admission_artifact
            .sha256,
    )
    .expect("retained original admission projection");
    assert_eq!(projection.repository_id, REPOSITORY_ID);
    assert_eq!(projection.head_repository_id, REPOSITORY_ID);
    assert_eq!(projection.source_sha, a.release.source_sha);
    assert_eq!(projection.run_id, 40_078_964_311);
    assert_eq!(projection.run_attempt, 2);
    assert_eq!(projection.ci_run_id, a.source_ci.run_id);
    assert_eq!(projection.ci_run_attempt, a.source_ci.attempt);
    assert_eq!(projection.commit_count, a.release.commit_count);
    assert_eq!(projection.version, a.release.version);
}

#[test]
fn historical_catalog_parser_rejects_duplicate_and_unknown_fields() {
    let plan = parse_package_release_plan(PLAN).expect("pinned canonical plan");
    let mut duplicate = A_RECORD[..A_RECORD.len() - 2].to_vec();
    duplicate.extend_from_slice(br#", "label":"D"}"#);
    assert!(parse_record(&duplicate, "A", &plan).is_err());

    let mut unknown = A_RECORD[..A_RECORD.len() - 2].to_vec();
    unknown.extend_from_slice(br#", "task_config":"ignored"}"#);
    assert!(parse_record(&unknown, "A", &plan).is_err());
}
