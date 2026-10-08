//! Validated parsing for server-returned Actions message-queue URLs.

/// The parts Host accepts from one absolute message-queue URL.
///
/// The origin is kept separately from the opaque route because requests are
/// assembled only after both the bound origin and request purpose are checked.
pub(super) struct QueueRouteParts {
    pub(super) origin: String,
    pub(super) path: String,
    pub(super) query: Option<String>,
}

/// Validate the returned HTTPS URL and split its origin from its path/query.
///
/// The host allowlist is an explicit transport policy for the documented
/// `*.actions.githubusercontent.com` service family; the pinned SDK itself
/// does not constrain queue-host equality. Path and raw-query validation is
/// completed by `MessageQueueRoute::from_parts` before the route is returned
/// to Session. Query text is kept out of this function's diagnostics.
pub(super) fn queue_route(value: &str) -> Option<QueueRouteParts> {
    if value.len() > 64 * 1024
        || value.bytes().any(|byte| {
            byte.is_ascii_whitespace() || byte.is_ascii_control() || matches!(byte, b'\\' | b'#')
        })
    {
        return None;
    }
    let rest = value.strip_prefix("https://")?;
    let (authority, route) = rest.split_once('/')?;
    if authority.contains('@') {
        return None;
    }
    let host = authority.strip_suffix(":443").unwrap_or(authority);
    if host.contains(':') || !allowed_queue_host(host) {
        return None;
    }
    let (path_suffix, query) = match route.split_once('?') {
        Some((path, query)) => (path, Some(query.to_owned())),
        None => (route, None),
    };
    if path_suffix.is_empty()
        || path_suffix.len() > 4095
        || query.as_ref().is_some_and(|q| q.len() > 8192)
    {
        return None;
    }
    Some(QueueRouteParts {
        origin: format!("https://{host}"),
        path: format!("/{path_suffix}"),
        query,
    })
}

fn allowed_queue_host(host: &str) -> bool {
    const SUFFIX: &str = ".actions.githubusercontent.com";
    let Some(prefix) = host.strip_suffix(SUFFIX) else {
        return false;
    };
    if prefix.is_empty() || host.len() > 253 || host.bytes().any(|byte| byte.is_ascii_uppercase()) {
        return false;
    }
    prefix.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            && label
                .as_bytes()
                .first()
                .is_some_and(u8::is_ascii_alphanumeric)
            && label
                .as_bytes()
                .last()
                .is_some_and(u8::is_ascii_alphanumeric)
    })
}

#[cfg(test)]
mod tests {
    use super::queue_route;

    #[test]
    fn queue_route_accepts_trusted_service_hosts_and_preserves_path_and_raw_query() {
        for host in [
            "pipelinesghubeus13.actions.githubusercontent.com",
            "pipelines.actions.githubusercontent.com",
            "queue-1.region.actions.githubusercontent.com",
            "pipelinesghubeus13.actions.githubusercontent.com:443",
        ] {
            let url =
                format!("https://{host}/_apis/runtime/messages?cursor=opaque%2f&x=mail@host.test");
            let route = queue_route(&url).expect("valid queue URL");
            let expected_origin = format!("https://{}", host.strip_suffix(":443").unwrap_or(host));
            assert_eq!(route.origin, expected_origin);
            assert_eq!(route.path, "/_apis/runtime/messages");
            assert_eq!(
                route.query.as_deref(),
                Some("cursor=opaque%2f&x=mail@host.test")
            );
        }
        let without_query = queue_route(
            "https://pipelinesghubeus13.actions.githubusercontent.com/_apis/runtime/messages",
        )
        .expect("valid path-only URL");
        assert_eq!(without_query.query, None);
    }

    #[test]
    fn queue_route_rejects_untrusted_authority_and_ambiguous_url_shapes() {
        for url in [
            "http://pipelinesghubeus13.actions.githubusercontent.com/messages",
            "https://actions.githubusercontent.com/messages",
            "https://pipelinesghubeus13.actions.githubusercontent.com.evil.test/messages",
            "https://user@pipelinesghubeus13.actions.githubusercontent.com/messages",
            "https://pipelinesghubeus13.actions.githubusercontent.com:444/messages",
            "https://Pipelinesghubeus13.actions.githubusercontent.com/messages",
            "https://pipelinesghubeus13.actions.githubusercontent.com",
        ] {
            assert!(queue_route(url).is_none(), "accepted {url}");
        }
        for url in [
            "https://pipelinesghubeus13.actions.githubusercontent.com/messages\\evil",
            "https://pipelinesghubeus13.actions.githubusercontent.com/messages#fragment",
            "https://pipelinesghubeus13.actions.githubusercontent.com/messages\n",
        ] {
            assert!(queue_route(url).is_none(), "accepted ambiguous URL {url:?}");
        }
    }
}
