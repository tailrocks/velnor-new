use velnor_runner_github::{Method, RequestPurpose, SessionRequest};

use super::{
    ActionsHeaders, ValidatedTarget, actions_delete_validation, headers_are, identity, session,
    validated_target,
};

pub(super) fn validate<'a>(
    path: &'a str,
    request: &'a SessionRequest,
) -> Option<ValidatedTarget<'a>> {
    let admin_headers = ActionsHeaders::AdminJsonBearer;
    match (request.method, path, request.query.as_deref()) {
        (Method::Get, "_apis/runtime/runnergroups", Some("api-version=6.0-preview")) => {
            if !identity::actions_request_is_valid(request) || !request.body.is_empty() {
                return None;
            }
            validated_target(
                path,
                request.query.as_deref(),
                headers_are(request, admin_headers),
            )
        }
        (Method::Get, "_apis/runtime/runnerscalesets", Some(query)) => validated_target(
            path,
            request.query.as_deref(),
            request.body.is_empty()
                && identity::actions_request_is_valid(request)
                && headers_are(request, admin_headers)
                && actions_delete_validation::valid_scale_set_query(query),
        ),
        (Method::Delete, path, Some(query)) if request.purpose == RequestPurpose::SessionClose => {
            validated_target(
                path,
                request.query.as_deref(),
                request.bearer_role == velnor_runner_github::BearerRole::ActionsAdmin
                    && request.body.is_empty()
                    && query == "api-version=6.0-preview"
                    && actions_delete_validation::session_delete_path(path)
                    && headers_are(request, ActionsHeaders::AdminSessionClose),
            )
        }
        _ => session::validate(path, request),
    }
}
