use std::collections::BTreeSet;

use velnor_runner_github::{BearerRole, Method, RequestPurpose, SessionRequest};

use super::ValidatedTarget;

const API_QUERY: &str = "api-version=6.0-preview";
const SCALE_SET_PREFIX: &str = "_apis/runtime/runnerscalesets/";

pub(super) fn validate<'a>(
    path: &'a str,
    request: &'a SessionRequest,
) -> Option<ValidatedTarget<'a>> {
    match (request.method, request.purpose, request.bearer_role) {
        (Method::Post, RequestPurpose::SessionCreate, BearerRole::ActionsAdmin)
            if session_collection(path)
                && request.query.as_deref() == Some(API_QUERY)
                && valid_owner_body(&request.body)
                && session_admin_headers(request) =>
        {
            Some(target(path, request))
        }
        (Method::Patch, RequestPurpose::SessionRefresh, BearerRole::ActionsAdmin)
            if session_item(path)
                && request.query.as_deref() == Some(API_QUERY)
                && request.body.is_empty()
                && session_admin_headers(request) =>
        {
            Some(target(path, request))
        }
        (Method::Post, RequestPurpose::AcquireJobs, BearerRole::SessionQueue)
            if scale_set_child(path, "/acquirejobs")
                && request.query.as_deref() == Some(API_QUERY)
                && valid_requested_ids(&request.body)
                && session_user_agent_headers(request, BearerRole::SessionQueue) =>
        {
            Some(target(path, request))
        }
        (Method::Post, RequestPurpose::GenerateJitConfig, BearerRole::ActionsAdmin)
            if scale_set_child(path, "/generatejitconfig")
                && request.query.as_deref() == Some(API_QUERY)
                && valid_jit_body(&request.body)
                && session_user_agent_headers(request, BearerRole::ActionsAdmin) =>
        {
            Some(target(path, request))
        }
        _ => None,
    }
}

fn target<'a>(path: &'a str, request: &'a SessionRequest) -> ValidatedTarget<'a> {
    ValidatedTarget {
        path,
        query: request.query.as_deref(),
    }
}

fn session_admin_headers(request: &SessionRequest) -> bool {
    exact_session_headers(request, false)
}

fn session_user_agent_headers(request: &SessionRequest, role: BearerRole) -> bool {
    matches!(role, BearerRole::ActionsAdmin | BearerRole::SessionQueue)
        && exact_session_headers(request, true)
}

fn exact_session_headers(request: &SessionRequest, user_agent: bool) -> bool {
    let mut authorization = None;
    let mut content_type = None;
    let mut observed_user_agent = None;
    for (name, value) in &request.headers {
        if value.bytes().any(|byte| byte.is_ascii_control()) {
            return false;
        }
        let slot = if name.eq_ignore_ascii_case("authorization") {
            &mut authorization
        } else if name.eq_ignore_ascii_case("content-type") {
            &mut content_type
        } else if name.eq_ignore_ascii_case("user-agent") {
            &mut observed_user_agent
        } else {
            return false;
        };
        if slot.replace(value.as_str()).is_some() {
            return false;
        }
    }
    request.headers.len() == if user_agent { 3 } else { 2 }
        && bearer_header(authorization)
        && content_type == Some("application/json")
        && observed_user_agent == user_agent.then_some("velnor-host")
}

fn bearer_header(value: Option<&str>) -> bool {
    let Some(token) = value.and_then(|value| value.strip_prefix("Bearer ")) else {
        return false;
    };
    !token.is_empty()
        && token.len() <= 8 * 1024
        && token.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}

fn valid_owner_body(body: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    if object.len() != 1 {
        return false;
    }
    object
        .get("ownerName")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|owner| {
            !owner.is_empty()
                && owner.len() <= 256
                && !owner.bytes().any(|byte| byte.is_ascii_control())
        })
}

fn valid_requested_ids(body: &[u8]) -> bool {
    let Ok(ids) = serde_json::from_slice::<Vec<i64>>(body) else {
        return false;
    };
    !ids.is_empty()
        && ids.len() <= 4096
        && ids.iter().all(|id| *id > 0)
        && ids.iter().copied().collect::<BTreeSet<_>>().len() == ids.len()
}

fn valid_jit_body(body: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(body) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    if object.len() != 2
        || object.get("workFolder").and_then(serde_json::Value::as_str) != Some("_work")
    {
        return false;
    }
    object
        .get("name")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|name| {
            !name.is_empty()
                && name.len() <= 255
                && !name.chars().any(|ch| ch.is_whitespace() || ch == '/')
        })
}

fn session_collection(path: &str) -> bool {
    let Some(scale_set_id) = path
        .strip_prefix(SCALE_SET_PREFIX)
        .and_then(|suffix| suffix.strip_suffix("/sessions"))
    else {
        return false;
    };
    positive_canonical_i64(scale_set_id)
}

fn session_item(path: &str) -> bool {
    let Some((prefix, session_id)) = path.rsplit_once("/sessions/") else {
        return false;
    };
    let Some(scale_set_id) = prefix.strip_prefix(SCALE_SET_PREFIX) else {
        return false;
    };
    positive_canonical_i64(scale_set_id) && safe_session_id(session_id)
}

fn scale_set_child(path: &str, suffix: &str) -> bool {
    let Some(scale_set_id) = path
        .strip_prefix(SCALE_SET_PREFIX)
        .and_then(|suffix_path| suffix_path.strip_suffix(suffix))
    else {
        return false;
    };
    positive_canonical_i64(scale_set_id)
}

fn positive_canonical_i64(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value
            .parse::<i64>()
            .is_ok_and(|parsed| parsed > 0 && parsed.to_string() == value)
}

fn safe_session_id(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && value.len() <= 256
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~'))
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
