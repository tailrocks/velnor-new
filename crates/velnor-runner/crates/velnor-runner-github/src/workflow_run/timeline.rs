//! Complete base-ref history for one pull request.

use std::collections::HashSet;

use super::{RepositoryTarget, WorkflowRunError, api_headers};
use crate::TransportFail;
use crate::session::{Exchange, Method, SessionRequest, Transport};

mod dto;
mod order;

pub use dto::{BaseRefChangedEvent, PullRequestBaseHistory};
use dto::{GraphQlRequest, GraphQlResponse, Snapshot, TimelineNode, Variables};
pub use order::{new_retarget_event, ordered_base_changes, retarget_event_after_acknowledgement};

const PAGE_SIZE: usize = 100;
const MAX_PAGES: usize = 64;
const MAX_EVENTS: usize = PAGE_SIZE * MAX_PAGES;
const MAX_CURSOR_BYTES: usize = 4096;
const QUERY: &str = "query PullRequestBaseHistory($owner:String!,$name:String!,$number:Int!,$after:String){repository(owner:$owner,name:$name){databaseId name owner{login} pullRequest(number:$number){number updatedAt headRefName headRefOid headRepository{databaseId name owner{login}} baseRefName baseRefOid baseRepository{databaseId name owner{login}} timelineItems(first:100,after:$after,itemTypes:[BASE_REF_CHANGED_EVENT]){nodes{__typename ... on BaseRefChangedEvent{id databaseId createdAt previousRefName currentRefName pullRequest{number repository{databaseId name owner{login}}}}} pageInfo{hasNextPage endCursor}}}}}";

/// Read every base-ref change for one authenticated pull request.
///
/// # Errors
///
/// Gives [`WorkflowRunError`] when identity, pagination, transport, or response data is incorrect.
pub fn get_pull_request_base_history<T>(
    transport: &mut T,
    expected: RepositoryTarget<'_>,
    number: i64,
    token: &str,
) -> Result<PullRequestBaseHistory, WorkflowRunError>
where
    T: Transport + ?Sized,
{
    validate_input(expected, number)?;
    let mut cursor: Option<String> = None;
    let mut seen_cursors = HashSet::new();
    let mut history: Option<PullRequestBaseHistory> = None;
    for _ in 0..MAX_PAGES {
        let page = fetch_page(transport, expected, number, cursor.as_deref(), token)?;
        if let Some(current) = history.as_mut() {
            if !same_snapshot(current, &page.history) {
                return Err(WorkflowRunError::Malformed);
            }
            append_events(current, page.history.events)?;
        } else {
            history = Some(page.history);
        }
        if !page.has_next_page {
            let complete = history.ok_or(WorkflowRunError::Malformed)?;
            ordered_base_changes(&complete)?;
            return Ok(complete);
        }
        let next = page.end_cursor.ok_or(WorkflowRunError::Malformed)?;
        if next.is_empty() || next.len() > MAX_CURSOR_BYTES || !seen_cursors.insert(next.clone()) {
            return Err(WorkflowRunError::Malformed);
        }
        cursor = Some(next);
    }
    Err(WorkflowRunError::ResponseTooLarge)
}

fn validate_input(expected: RepositoryTarget<'_>, number: i64) -> Result<(), WorkflowRunError> {
    if expected.id <= 0
        || number <= 0
        || expected.owner.is_empty()
        || expected.name.is_empty()
        || !path_part(expected.owner)
        || !path_part(expected.name)
    {
        Err(WorkflowRunError::InvalidRequest)
    } else {
        Ok(())
    }
}

fn fetch_page<T>(
    transport: &mut T,
    expected: RepositoryTarget<'_>,
    number: i64,
    after: Option<&str>,
    token: &str,
) -> Result<Snapshot, WorkflowRunError>
where
    T: Transport + ?Sized,
{
    let body = serde_json::to_vec(&GraphQlRequest {
        query: QUERY,
        variables: Variables {
            owner: expected.owner,
            name: expected.name,
            number,
            after,
        },
    })
    .map_err(|_| WorkflowRunError::InvalidRequest)?;
    let mut headers = api_headers(token)?;
    headers.push(("Content-Type".to_owned(), "application/json".to_owned()));
    let request = SessionRequest {
        method: Method::Post,
        path: "/graphql".to_owned(),
        query: None,
        headers,
        body,
    };
    let exchange = match transport.exchange(&request) {
        Ok(exchange) => exchange,
        Err(TransportFail::Timeout | TransportFail::Reset) => {
            return Err(WorkflowRunError::Uncertain);
        }
        Err(TransportFail::Http(status)) => return Err(super::api_status(status)),
    };
    decode_page(&exchange, expected, number)
}

fn decode_page(
    exchange: &Exchange,
    expected: RepositoryTarget<'_>,
    number: i64,
) -> Result<Snapshot, WorkflowRunError> {
    if exchange.status != 200 {
        return Err(super::api_status(exchange.status));
    }
    if exchange.body.len() > super::MAX_RESPONSE_BYTES {
        return Err(WorkflowRunError::ResponseTooLarge);
    }
    let response: GraphQlResponse =
        serde_json::from_slice(&exchange.body).map_err(|_| WorkflowRunError::Malformed)?;
    if !response.errors.is_empty() {
        return Err(WorkflowRunError::Uncertain);
    }
    let repository = response
        .data
        .and_then(|data| data.repository)
        .ok_or(WorkflowRunError::Malformed)?;
    let repository_id = repository.database_id.ok_or(WorkflowRunError::Malformed)?;
    if repository_id != expected.id
        || !same_name(&repository.owner.login, expected.owner)
        || !same_name(&repository.name, expected.name)
    {
        return Err(WorkflowRunError::RepositoryMismatch);
    }
    let pull = repository.pull_request.ok_or(WorkflowRunError::Malformed)?;
    if pull.number != number
        || pull.head_ref_name.is_empty()
        || pull.base_ref_name.is_empty()
        || !valid_sha(&pull.head_ref_oid)
        || !valid_sha(&pull.base_ref_oid)
        || pull.updated_at.is_empty()
    {
        return Err(WorkflowRunError::Malformed);
    }
    let head_repository = pull.head_repository.ok_or(WorkflowRunError::Malformed)?;
    let base_repository = pull.base_repository.ok_or(WorkflowRunError::Malformed)?;
    let head_repository_id = head_repository
        .database_id
        .ok_or(WorkflowRunError::Malformed)?;
    let base_repository_id = base_repository
        .database_id
        .ok_or(WorkflowRunError::Malformed)?;
    if head_repository_id <= 0
        || base_repository_id != expected.id
        || !same_name(&base_repository.owner.login, expected.owner)
        || !same_name(&base_repository.name, expected.name)
        || head_repository.name.is_empty()
        || head_repository.owner.login.is_empty()
    {
        return Err(WorkflowRunError::RepositoryMismatch);
    }
    let events = pull
        .timeline_items
        .nodes
        .into_iter()
        .map(|node| decode_event(node, number, expected))
        .collect::<Result<Vec<_>, _>>()?;
    if events.len() > MAX_EVENTS {
        return Err(WorkflowRunError::ResponseTooLarge);
    }
    Ok(Snapshot {
        history: PullRequestBaseHistory {
            number,
            repository_id,
            head_repository_id,
            head_ref: pull.head_ref_name,
            head_sha: pull.head_ref_oid,
            base_repository_id,
            base_ref: pull.base_ref_name,
            base_sha: pull.base_ref_oid,
            updated_at: pull.updated_at,
            events,
        },
        has_next_page: pull.timeline_items.page_info.has_next_page,
        end_cursor: pull.timeline_items.page_info.end_cursor,
    })
}

fn decode_event(
    node: TimelineNode,
    number: i64,
    expected: RepositoryTarget<'_>,
) -> Result<BaseRefChangedEvent, WorkflowRunError> {
    let event_repository = node
        .pull_request
        .repository
        .ok_or(WorkflowRunError::Malformed)?;
    if node.kind != "BaseRefChangedEvent"
        || node.pull_request.number != number
        || event_repository.database_id != Some(expected.id)
        || !same_name(&event_repository.owner.login, expected.owner)
        || !same_name(&event_repository.name, expected.name)
        || node.id.is_empty()
        || node.database_id.is_some_and(|id| id <= 0)
    {
        return Err(WorkflowRunError::Malformed);
    }
    Ok(BaseRefChangedEvent {
        id: node.id,
        database_id: node.database_id,
        created_at: node.created_at,
        previous_ref_name: node.previous_ref_name,
        current_ref_name: node.current_ref_name,
    })
}

fn append_events(
    history: &mut PullRequestBaseHistory,
    events: Vec<BaseRefChangedEvent>,
) -> Result<(), WorkflowRunError> {
    if history.events.len().saturating_add(events.len()) > MAX_EVENTS {
        return Err(WorkflowRunError::ResponseTooLarge);
    }
    let mut ids: HashSet<String> = history
        .events
        .iter()
        .map(|event| event.id.clone())
        .collect();
    for event in events {
        if !ids.insert(event.id.clone()) {
            return Err(WorkflowRunError::Malformed);
        }
        history.events.push(event);
    }
    Ok(())
}

fn same_snapshot(left: &PullRequestBaseHistory, right: &PullRequestBaseHistory) -> bool {
    left.number == right.number
        && left.repository_id == right.repository_id
        && left.head_repository_id == right.head_repository_id
        && left.head_ref == right.head_ref
        && left.head_sha == right.head_sha
        && left.base_repository_id == right.base_repository_id
        && left.base_ref == right.base_ref
        && left.base_sha == right.base_sha
        && left.updated_at == right.updated_at
}

fn same_name(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn valid_sha(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn path_part(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}
