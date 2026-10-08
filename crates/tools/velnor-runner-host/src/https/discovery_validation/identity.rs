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
        Method::Get if workflow_run_path(path) || attempt_jobs_path(path) => (
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

pub(super) fn actions_request_is_valid(request: &SessionRequest) -> bool {
    request.bearer_role == BearerRole::ActionsAdmin
        && matches!(
            (request.method, request.purpose),
            (Method::Get, RequestPurpose::ActionsMetadataRead)
                | (Method::Delete, RequestPurpose::SessionClose)
        )
}
