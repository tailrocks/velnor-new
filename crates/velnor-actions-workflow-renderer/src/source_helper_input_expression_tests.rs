use super::{prepared_binding, source_binding, validate_operation};
use std::collections::BTreeMap;
use velnor_actions_contract::SourceBoundOperation::{
    RustReleasePackageVerify, RustReleasePreparedPackage, RustReleaseSourceSnapshot,
};

#[test]
fn source_inputs_admit_only_exact_prepared_and_verify_bindings() {
    for key in [
        "RELEASE_SOURCE_SNAPSHOT_ARTIFACT_ID",
        "RELEASE_SOURCE_SNAPSHOT_ARTIFACT_DIGEST",
        "RELEASE_SOURCE_SNAPSHOT_BLOB_SHA256",
        "RELEASE_SOURCE_COMMIT_SHA",
        "RELEASE_SOURCE_TREE_SHA",
        "GITHUB_REF",
        "GITHUB_WORKFLOW_REF",
        "GITHUB_WORKFLOW_SHA",
    ] {
        let inner = source_binding(key).expect("source binding");
        let value = format!("${{{{ {inner} }}}}");
        for operation in [RustReleasePreparedPackage, RustReleasePackageVerify] {
            let exact = BTreeMap::from([(key.to_owned(), value.clone())]);
            assert!(validate_operation(operation, &exact).is_ok());
            for bad in [
                format!("prefix{value}"),
                "${{ github.token }}".to_owned(),
                value.replace("release-source-snapshot", "foreign"),
                String::new(),
            ] {
                if bad != value {
                    assert!(
                        validate_operation(operation, &BTreeMap::from([(key.to_owned(), bad)]))
                            .is_err()
                    );
                }
            }
            assert!(
                validate_operation(
                    operation,
                    &BTreeMap::from([("OTHER".to_owned(), value.clone())])
                )
                .is_err()
            );
        }
        assert!(
            validate_operation(
                RustReleaseSourceSnapshot,
                &BTreeMap::from([(key.to_owned(), value)])
            )
            .is_err()
        );
    }
}

#[test]
fn prepared_artifact_inputs_belong_only_to_verifier() {
    for key in [
        "RELEASE_PACKAGE_ARTIFACT_ID",
        "RELEASE_PACKAGE_ARTIFACT_DIGEST",
        "RELEASE_PACKAGE_BLOB_SHA256",
    ] {
        let inner = prepared_binding(key).expect("prepared binding");
        let exact = BTreeMap::from([(key.to_owned(), format!("${{{{ {inner} }}}}"))]);
        assert!(validate_operation(RustReleasePackageVerify, &exact).is_ok());
        assert!(validate_operation(RustReleasePreparedPackage, &exact).is_err());
        let foreign = BTreeMap::from([(
            key.to_owned(),
            format!("${{{{ {} }}}}", inner.replace("release-package", "foreign")),
        )]);
        assert!(validate_operation(RustReleasePackageVerify, &foreign).is_err());
    }
}

#[test]
fn scoped_bindings_preserve_reserved_structural_and_expression_checks() {
    for (key, value) in [
        ("GITHUB_OUTPUT", "/tmp/output"),
        ("VELNOR_COMPILED_HELPER_SOURCE", "data"),
        ("bad-key", "data"),
        ("OTHER", "bad\nvalue"),
        ("OTHER", "${{ env.FOREIGN }}"),
    ] {
        assert!(
            validate_operation(
                RustReleasePreparedPackage,
                &BTreeMap::from([(key.to_owned(), value.to_owned())])
            )
            .is_err()
        );
    }
    assert!(
        validate_operation(
            RustReleasePreparedPackage,
            &BTreeMap::from([("RUNNER_TEMP".to_owned(), "${{ runner.temp }}".to_owned())])
        )
        .is_ok()
    );
}
