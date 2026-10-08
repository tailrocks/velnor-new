use velnor_runner_github::{BearerRole, Method, RequestPurpose, SessionRequest};

use super::super::QueueOrigin;
use super::ValidatedTarget;

const POLL_ACCEPT: &str = "application/json; api-version=6.0-preview";

pub(super) fn validate<'a>(
    origin: &QueueOrigin,
    request: &'a SessionRequest,
) -> Option<ValidatedTarget<'a>> {
    let valid = match (request.method, request.purpose, request.bearer_role) {
        (Method::Get, RequestPurpose::MessageQueuePoll, BearerRole::SessionQueue) => {
            origin
                .route
                .matches_poll_target(&request.path, request.query.as_deref())
                && request.body.is_empty()
                && poll_headers(request)
        }
        (Method::Delete, RequestPurpose::MessageAcknowledge, BearerRole::SessionQueue) => {
            origin
                .route
                .matches_ack_target(&request.path, request.query.as_deref())
                && request.body.is_empty()
                && acknowledge_headers(request)
        }
        _ => false,
    };
    if !valid {
        return None;
    }
    let path = request.path.strip_prefix('/')?;
    Some(ValidatedTarget {
        path,
        query: request.query.as_deref(),
    })
}

fn poll_headers(request: &SessionRequest) -> bool {
    let mut authorization = None;
    let mut accept = None;
    let mut user_agent = None;
    let mut capacity = None;
    for (name, value) in &request.headers {
        if value.bytes().any(|byte| byte.is_ascii_control()) {
            return false;
        }
        let slot = if name.eq_ignore_ascii_case("authorization") {
            &mut authorization
        } else if name.eq_ignore_ascii_case("accept") {
            &mut accept
        } else if name.eq_ignore_ascii_case("user-agent") {
            &mut user_agent
        } else if name.eq_ignore_ascii_case(velnor_runner_github::CAPACITY_HEADER) {
            &mut capacity
        } else {
            return false;
        };
        if slot.replace(value.as_str()).is_some() {
            return false;
        }
    }
    request.headers.len() == 4
        && super::auth_scheme(authorization, "Bearer")
        && accept == Some(POLL_ACCEPT)
        && user_agent == Some("velnor-host")
        && capacity.is_some_and(canonical_capacity)
}

fn canonical_capacity(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value
            .parse::<u32>()
            .is_ok_and(|parsed| parsed.to_string() == value)
}

fn acknowledge_headers(request: &SessionRequest) -> bool {
    let mut authorization = None;
    let mut content_type = None;
    let mut user_agent = None;
    for (name, value) in &request.headers {
        if value.bytes().any(|byte| byte.is_ascii_control()) {
            return false;
        }
        let slot = if name.eq_ignore_ascii_case("authorization") {
            &mut authorization
        } else if name.eq_ignore_ascii_case("content-type") {
            &mut content_type
        } else if name.eq_ignore_ascii_case("user-agent") {
            &mut user_agent
        } else {
            return false;
        };
        if slot.replace(value.as_str()).is_some() {
            return false;
        }
    }
    request.headers.len() == 3
        && super::auth_scheme(authorization, "Bearer")
        && content_type == Some("application/json")
        && user_agent == Some("velnor-host")
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
