use super::{github_environment, record, validate_record};
use velnor_actions_contract::SourceBoundOperation;
use velnor_actions_contract::workflow::native_tools::NativeCredentialScope;

#[test]
fn issue_write_scope_admits_only_the_fixed_observer() {
    let exact = record(
        SourceBoundOperation::VerificationObserver,
        NativeCredentialScope::GithubIssueWrite,
        github_environment(),
    );
    assert!(validate_record(&exact).is_ok());

    for operation in [
        SourceBoundOperation::NativePagesAdmission,
        SourceBoundOperation::RustForgePublish,
    ] {
        assert!(
            validate_record(&record(
                operation,
                NativeCredentialScope::GithubIssueWrite,
                github_environment(),
            ))
            .is_err()
        );
    }
    for scope in [
        NativeCredentialScope::GithubReadOnly,
        NativeCredentialScope::GithubReleasePublish,
    ] {
        assert!(
            validate_record(&record(
                SourceBoundOperation::VerificationObserver,
                scope,
                github_environment(),
            ))
            .is_err()
        );
    }
}

#[test]
fn issue_write_scope_requires_exact_token_and_no_extra_credentials() {
    for environment in [
        std::collections::BTreeMap::from([(
            String::from("GH_TOKEN"),
            String::from("${{ secrets.OBSERVER_TOKEN }}"),
        )]),
        std::collections::BTreeMap::from([
            (
                String::from("GH_TOKEN"),
                String::from("${{ github.token }}"),
            ),
            (String::from("GITHUB_OUTPUT"), String::from("/tmp/output")),
        ]),
    ] {
        assert!(
            validate_record(&record(
                SourceBoundOperation::VerificationObserver,
                NativeCredentialScope::GithubIssueWrite,
                environment,
            ))
            .is_err()
        );
    }
}
