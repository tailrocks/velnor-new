//! `GET _apis/runtime/runnergroups`. Ids and names only.

use serde::Deserialize;

use crate::session::{API_QUERY, bearer, execute, json_content, user_agent};
use crate::{Method, SessionError, SessionRequest, Transport, WireError};

use super::other_status;

/// One runner group. No URL and no token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunnerGroup {
    /// Service id.
    pub id: i64,
    /// Group name.
    pub name: String,
    /// `isDefaultGroup` from the service.
    pub is_default: bool,
}

/// List runner groups on the current Actions origin.
///
/// # Errors
///
/// Returns [`WireError::RegistrationRejected`] for an empty admin token.
/// A non-200 status is the pinned status class. A `count` that does not match
/// `value` is [`WireError::Malformed`].
pub fn list_runner_groups<T>(
    transport: &mut T,
    admin_token: &str,
) -> Result<Vec<RunnerGroup>, SessionError>
where
    T: Transport + ?Sized,
{
    if admin_token.is_empty() {
        return Err(SessionError::Wire(WireError::RegistrationRejected));
    }
    let request = SessionRequest {
        method: Method::Get,
        path: "_apis/runtime/runnergroups".to_owned(),
        query: Some(API_QUERY.to_owned()),
        headers: vec![json_content(), bearer(admin_token)?, user_agent()],
        body: Vec::new(),
    };
    let exchange = execute(transport, &request)?;
    if exchange.status != 200 {
        return Err(other_status(exchange.status));
    }
    decode_groups(&exchange.body)
}

fn decode_groups(body: &[u8]) -> Result<Vec<RunnerGroup>, SessionError> {
    let page: GroupPage = serde_json::from_slice(body).map_err(|_| WireError::Malformed)?;
    let len = i64::try_from(page.value.len()).map_err(|_| WireError::Malformed)?;
    if page.count != len {
        return Err(SessionError::from(WireError::Malformed));
    }
    Ok(page
        .value
        .into_iter()
        .map(|item| RunnerGroup {
            id: item.id,
            name: item.name,
            is_default: item.is_default,
        })
        .collect())
}

#[derive(Deserialize)]
struct GroupPage {
    count: i64,
    #[serde(default)]
    value: Vec<GroupItem>,
}

#[derive(Deserialize)]
struct GroupItem {
    id: i64,
    name: String,
    #[serde(default, rename = "isDefaultGroup")]
    is_default: bool,
}
