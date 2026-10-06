use super::super::credentials::validate_record;
use super::record;
use std::collections::BTreeMap;
use velnor_actions_contract::SourceBoundOperation;
use velnor_actions_contract::workflow::native_tools::NativeCredentialScope;

const API_KEY_PATH: &str = "${{ runner.temp }}/velnor/apple/AuthKey.p8";

fn environment() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("EXPECTED_TEAM_ID".to_owned(), "ABCDE12345".to_owned()),
        (
            "EXPECTED_CERT_SHA256".to_owned(),
            "abcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcdefabcd".to_owned(),
        ),
        (
            "DEVELOPER_ID_APPLICATION".to_owned(),
            "${{ secrets.DEVELOPER_ID_APPLICATION }}".to_owned(),
        ),
        (
            "DEVELOPER_ID_APPLICATION_P12_BASE64".to_owned(),
            "${{ secrets.DEVELOPER_ID_APPLICATION_P12_BASE64 }}".to_owned(),
        ),
        (
            "DEVELOPER_ID_APPLICATION_P12_PASSWORD".to_owned(),
            "${{ secrets.DEVELOPER_ID_APPLICATION_P12_PASSWORD }}".to_owned(),
        ),
        (
            "APP_STORE_CONNECT_API_KEY_P8".to_owned(),
            "${{ secrets.APP_STORE_CONNECT_API_KEY_P8 }}".to_owned(),
        ),
        (
            "APP_STORE_CONNECT_API_KEY_PATH".to_owned(),
            API_KEY_PATH.to_owned(),
        ),
        (
            "APP_STORE_CONNECT_KEY_ID".to_owned(),
            "${{ secrets.APP_STORE_CONNECT_KEY_ID }}".to_owned(),
        ),
        (
            "APP_STORE_CONNECT_ISSUER_ID".to_owned(),
            "${{ secrets.APP_STORE_CONNECT_ISSUER_ID }}".to_owned(),
        ),
    ])
}

#[test]
fn apple_scope_requires_exact_native_swift_bindings() {
    let exact = environment();
    assert!(
        validate_record(&record(
            SourceBoundOperation::NativeSwiftExecution,
            NativeCredentialScope::AppleSigning,
            exact.clone(),
        ))
        .is_ok()
    );

    assert!(
        validate_record(&record(
            SourceBoundOperation::NativePagesAdmission,
            NativeCredentialScope::AppleSigning,
            exact.clone(),
        ))
        .is_err()
    );

    let mut wrong_path = exact.clone();
    wrong_path.insert(
        "APP_STORE_CONNECT_API_KEY_PATH".to_owned(),
        "${{ runner.temp }}/foreign/AuthKey.p8".to_owned(),
    );
    assert!(
        validate_record(&record(
            SourceBoundOperation::NativeSwiftExecution,
            NativeCredentialScope::AppleSigning,
            wrong_path,
        ))
        .is_err()
    );

    let mut missing = exact;
    missing.remove("APP_STORE_CONNECT_ISSUER_ID");
    assert!(
        validate_record(&record(
            SourceBoundOperation::NativeSwiftExecution,
            NativeCredentialScope::AppleSigning,
            missing,
        ))
        .is_err()
    );
}

#[test]
fn apple_scope_rejects_anonymous_and_foreign_operation_bindings() {
    assert!(
        validate_record(&record(
            SourceBoundOperation::NativePagesAdmission,
            NativeCredentialScope::AppleSigning,
            BTreeMap::new(),
        ))
        .is_err()
    );
}
