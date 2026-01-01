use velnor_runner_github::{BearerRole, Method, RequestPurpose, SessionRequest};

use super::validate;

fn request(
    purpose: RequestPurpose,
    bearer_role: BearerRole,
    method: Method,
    path: &str,
    body: &[u8],
    user_agent: bool,
) -> SessionRequest {
    let mut headers = vec![
        ("Content-Type".to_owned(), "application/json".to_owned()),
        (
            "Authorization".to_owned(),
            "Bearer fixed-synthetic-marker".to_owned(),
        ),
    ];
    if user_agent {
        headers.push(("User-Agent".to_owned(), "velnor-host".to_owned()));
    }
    SessionRequest {
        purpose,
        bearer_role,
        method,
        path: path.to_owned(),
        query: Some("api-version=6.0-preview".to_owned()),
        headers,
        body: body.to_vec(),
    }
}

#[test]
fn session_create_refresh_acquire_and_jit_routes_require_exact_typed_contracts() {
    let cases = [
        request(
            RequestPurpose::SessionCreate,
            BearerRole::ActionsAdmin,
            Method::Post,
            "_apis/runtime/runnerscalesets/42/sessions",
            br#"{"ownerName":"worker-1"}"#,
            false,
        ),
        request(
            RequestPurpose::SessionRefresh,
            BearerRole::ActionsAdmin,
            Method::Patch,
            "_apis/runtime/runnerscalesets/42/sessions/session~1",
            b"",
            false,
        ),
        request(
            RequestPurpose::AcquireJobs,
            BearerRole::SessionQueue,
            Method::Post,
            "_apis/runtime/runnerscalesets/42/acquirejobs",
            b"[23,24]",
            true,
        ),
        request(
            RequestPurpose::GenerateJitConfig,
            BearerRole::ActionsAdmin,
            Method::Post,
            "_apis/runtime/runnerscalesets/42/generatejitconfig",
            br#"{"name":"worker-1","workFolder":"_work"}"#,
            true,
        ),
    ];
    for request in &cases {
        let path = request.path.as_str();
        let target = validate(path, request).expect("typed session request");
        assert_eq!(target.path, path);
        assert_eq!(target.query, Some("api-version=6.0-preview"));
    }
}

#[test]
fn session_route_rejects_wrong_method_purpose_bearer_query_headers_and_body() {
    let valid = request(
        RequestPurpose::SessionRefresh,
        BearerRole::ActionsAdmin,
        Method::Patch,
        "_apis/runtime/runnerscalesets/42/sessions/session-1",
        b"",
        false,
    );
    let mut wrong_method = valid.clone();
    wrong_method.method = Method::Post;
    assert!(validate(&wrong_method.path, &wrong_method).is_none());
    let mut wrong_purpose = valid.clone();
    wrong_purpose.purpose = RequestPurpose::SessionCreate;
    assert!(validate(&wrong_purpose.path, &wrong_purpose).is_none());
    let mut wrong_bearer = valid.clone();
    wrong_bearer.bearer_role = BearerRole::SessionQueue;
    assert!(validate(&wrong_bearer.path, &wrong_bearer).is_none());
    let mut wrong_query = valid.clone();
    wrong_query.query = Some("api-version=7.0".to_owned());
    assert!(validate(&wrong_query.path, &wrong_query).is_none());
    let mut wrong_header = valid.clone();
    wrong_header
        .headers
        .push(("X-Extra".to_owned(), "no".to_owned()));
    assert!(validate(&wrong_header.path, &wrong_header).is_none());
    let mut nonempty_body = valid;
    nonempty_body.body = b"{}".to_vec();
    assert!(validate(&nonempty_body.path, &nonempty_body).is_none());
}

#[test]
fn session_paths_and_json_bodies_reject_ambiguous_or_unowned_targets() {
    let mut bad = request(
        RequestPurpose::SessionCreate,
        BearerRole::ActionsAdmin,
        Method::Post,
        "_apis/runtime/runnerscalesets/042/sessions",
        br#"{"ownerName":"worker-1"}"#,
        false,
    );
    assert!(validate(&bad.path, &bad).is_none());
    bad.path = "_apis/runtime/runnerscalesets/42/sessions".to_owned();
    bad.body = br#"{"ownerName":"worker-1","other":true}"#.to_vec();
    assert!(validate(&bad.path, &bad).is_none());

    let mut acquire = request(
        RequestPurpose::AcquireJobs,
        BearerRole::SessionQueue,
        Method::Post,
        "_apis/runtime/runnerscalesets/42/acquirejobs",
        b"[23,23]",
        true,
    );
    assert!(validate(&acquire.path, &acquire).is_none());
    acquire.body = b"[0]".to_vec();
    assert!(validate(&acquire.path, &acquire).is_none());

    let mut jit = request(
        RequestPurpose::GenerateJitConfig,
        BearerRole::ActionsAdmin,
        Method::Post,
        "_apis/runtime/runnerscalesets/42/generatejitconfig",
        br#"{"name":"worker/1","workFolder":"_work"}"#,
        true,
    );
    assert!(validate(&jit.path, &jit).is_none());
    jit.body = br#"{"name":"worker-1","workFolder":"/tmp"}"#.to_vec();
    assert!(validate(&jit.path, &jit).is_none());
}
