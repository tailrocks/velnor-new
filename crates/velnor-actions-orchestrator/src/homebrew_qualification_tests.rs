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
    assert!(admission(&a, &plan).is_ok());
    assert!(admission(&d, &plan).is_ok());
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
