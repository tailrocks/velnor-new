//! Construct the fixed GitHub REST request shapes used by Actions readers.

use super::{ACCEPT, API_VERSION};
use crate::{BearerRole, Method, RequestPurpose, SessionError, SessionRequest, WireError};

pub(crate) fn actions_request(
    path: String,
    actions_token: &str,
) -> Result<SessionRequest, SessionError> {
    actions_request_with_purpose(path, actions_token, RequestPurpose::ActionsRead)
}

pub(crate) fn actions_request_with_purpose(
    path: String,
    actions_token: &str,
    purpose: RequestPurpose,
) -> Result<SessionRequest, SessionError> {
    if !safe_actions_token(actions_token) {
        return Err(WireError::RegistrationRejected.into());
    }
    Ok(SessionRequest {
        purpose,
        bearer_role: BearerRole::GithubRestCredential,
        method: Method::Get,
        path,
        query: None,
        headers: vec![
            ("Accept".to_owned(), ACCEPT.to_owned()),
            (
                "Authorization".to_owned(),
                format!("Bearer {actions_token}"),
            ),
            ("X-GitHub-Api-Version".to_owned(), API_VERSION.to_owned()),
            ("User-Agent".to_owned(), "velnor-host".to_owned()),
        ],
        body: Vec::new(),
    })
}

pub(super) fn safe_actions_token(token: &str) -> bool {
    !token.is_empty()
        && !token
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
}
