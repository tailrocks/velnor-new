use crate::{OrchestratorError, internal::internal};
use std::{
    ffi::OsString,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const NONCE_KEY: &str = "_velnor_read_nonce";
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn arguments(route: &str) -> Result<Vec<OsString>, OrchestratorError> {
    validate_route(route)?;
    let sequence = SEQUENCE
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(1)
        })
        .map_err(|_| internal("github_read_nonce_exhausted"))?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| internal("github_read_clock"))?
        .as_nanos();
    let nonce = format!("{:x}-{nanos:x}-{sequence:x}", std::process::id());
    let separator = if route.contains('?') { '&' } else { '?' };
    Ok(vec![
        format!("{route}{separator}{NONCE_KEY}={nonce}").into(),
        "-H".into(),
        "Cache-Control: no-cache, no-store".into(),
        "-H".into(),
        "Pragma: no-cache".into(),
    ])
}

pub(crate) fn for_request(args: Vec<OsString>) -> Result<Vec<OsString>, OrchestratorError> {
    match args.as_slice() {
        [route] => {
            let route = route
                .to_str()
                .ok_or_else(|| internal("github_read_route_encoding"))?;
            return arguments(route);
        }
        [_, flag, method, fields @ ..]
            if flag == "--method"
                && matches!(method.to_str(), Some("POST" | "PATCH" | "PUT" | "DELETE"))
                && fields.as_chunks::<2>().1.is_empty()
                && fields.as_chunks::<2>().0.iter().all(|pair| {
                    matches!(
                        pair[0].to_str(),
                        Some("--field" | "--raw-field" | "--input")
                    )
                }) => {}
        [endpoint, flag, query, fields @ ..]
            if endpoint == "graphql"
                && flag == "--raw-field"
                && query
                    .to_str()
                    .is_some_and(|value| value.starts_with("query="))
                && fields.as_chunks::<2>().1.is_empty()
                && fields
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .all(|pair| matches!(pair[0].to_str(), Some("--field" | "--raw-field"))) => {}
        _ => return Err(internal("github_read_request_intent")),
    }
    Ok(args)
}

fn validate_route(route: &str) -> Result<(), OrchestratorError> {
    if route.is_empty()
        || route.starts_with('/')
        || route.contains("://")
        || route.contains(['#', '\\'])
        || route.bytes().any(|byte| !(0x21..0x7f).contains(&byte))
    {
        return Err(internal("github_read_route"));
    }
    if let Some((_, query)) = route.split_once('?') {
        for field in query.split('&') {
            let key = field.split('=').next().unwrap_or_default();
            if key.is_empty()
                || key.eq_ignore_ascii_case(NONCE_KEY)
                || !key
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            {
                return Err(internal("github_read_query"));
            }
        }
    }
    Ok(())
}
