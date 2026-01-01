use super::{
    admin_exchange_path, attempt_jobs_path, registration_path, repository_path, workflow_run_path,
};
use velnor_runner_github::{BearerRole, Method, RequestPurpose, SessionRequest};

pub(super) fn github_request_is_valid(path: &str, request: &SessionRequest) -> bool {
    let expected = match request.method {
        Method::Get if repository_path(path) => (
            RequestPurpose::RepositoryRead,
            BearerRole::GithubRestCredential,
        ),
        Method::Get if workflow_run_read_path(path) || workflow_run_page_path(path) => (
            RequestPurpose::ActionsRead,
            BearerRole::GithubRestCredential,
        ),
        Method::Post if registration_path(path) => (
            RequestPurpose::RegistrationTokenIssue,
            BearerRole::GithubRestCredential,
        ),
        Method::Post if admin_exchange_path(path) => (
            RequestPurpose::ActionsAdminExchange,
            BearerRole::RegistrationToken,
        ),
        _ => return false,
    };
    request.purpose == expected.0 && request.bearer_role == expected.1
}

pub(super) fn workflow_run_read_path(path: &str) -> bool {
    workflow_run_path(path) || attempt_metadata_path(path)
}

pub(super) fn workflow_run_page_path(path: &str) -> bool {
    attempt_jobs_path(path) || run_artifacts_path(path)
}

fn attempt_metadata_path(path: &str) -> bool {
    let mut parts = path.split('/');
    matches!(parts.next(), Some("repos"))
        && parts.next().is_some_and(super::path_segment)
        && parts.next().is_some_and(super::path_segment)
        && matches!(parts.next(), Some("actions"))
        && matches!(parts.next(), Some("runs"))
        && parts.next().is_some_and(super::positive_canonical_u64)
        && matches!(parts.next(), Some("attempts"))
        && parts.next().is_some_and(super::positive_canonical_u64)
        && parts.next().is_none()
}

fn run_artifacts_path(path: &str) -> bool {
    let mut parts = path.split('/');
    matches!(parts.next(), Some("repos"))
        && parts.next().is_some_and(super::path_segment)
        && parts.next().is_some_and(super::path_segment)
        && matches!(parts.next(), Some("actions"))
        && matches!(parts.next(), Some("runs"))
        && parts.next().is_some_and(super::positive_canonical_u64)
        && matches!(parts.next(), Some("artifacts"))
        && parts.next().is_none()
}

pub(super) fn actions_request_is_valid(request: &SessionRequest) -> bool {
    request.bearer_role == BearerRole::ActionsAdmin
        && matches!(
            (request.method, request.purpose),
            (Method::Get, RequestPurpose::ActionsMetadataRead)
                | (Method::Delete, RequestPurpose::SessionClose)
        )
}
