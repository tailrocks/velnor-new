// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) fn valid_headers(headers: &ureq::http::HeaderMap, limit: u64) -> bool {
    let mut encodings = headers.get_all("content-encoding").iter();
    match (encodings.next(), encodings.next()) {
        (None, None) => {}
        (Some(value), None)
            if value
                .to_str()
                .is_ok_and(|value| value.eq_ignore_ascii_case("identity")) => {}
        _ => return false,
    }
    let mut lengths = headers.get_all("content-length").iter();
    match (lengths.next(), lengths.next()) {
        (None, None) => true,
        (Some(value), None) => value
            .to_str()
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .is_some_and(|n| n <= limit),
        _ => false,
    }
}

pub(crate) fn encode(value: &str) -> String {
    let mut result = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            result.push(char::from(byte));
        } else {
            const HEX: &[u8; 16] = b"0123456789ABCDEF";
            result.push('%');
            result.push(char::from(HEX[usize::from(byte >> 4)]));
            result.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    result
}

pub(super) fn valid_sha(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub(super) fn valid_segment(value: &str) -> bool {
    if value.is_empty() || value.len() > 768 {
        return false;
    }
    let mut decoded = Vec::new();
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let (Some(a), Some(b)) = (bytes.next(), bytes.next()) else {
                return false;
            };
            let Some(a) = char::from(a).to_digit(16) else {
                return false;
            };
            let Some(b) = char::from(b).to_digit(16) else {
                return false;
            };
            let value = u8::try_from(a * 16 + b).unwrap_or(0);
            if value == b'%' || value.is_ascii_control() || matches!(value, b'?' | b'#' | b'\\') {
                return false;
            }
            decoded.push(value);
        } else if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'+') {
            decoded.push(byte);
        } else {
            return false;
        }
    }
    let Ok(text) = std::str::from_utf8(&decoded) else {
        return false;
    };
    !text
        .split('/')
        .any(|part| part == "." || part == ".." || part.is_empty())
}

pub(super) fn numeric(value: &str) -> bool {
    value.parse::<u64>().is_ok_and(|n| n > 0)
}

pub(super) fn page_query(query: &str) -> bool {
    query
        .strip_prefix("per_page=100&page=")
        .is_some_and(|v| v.parse::<u32>().is_ok_and(|n| (1..=100).contains(&n)))
}

pub(super) fn valid_post_route(route: &str) -> bool {
    let parts: Vec<_> = route.split('/').collect();
    let ["repos", owner, repo, rest @ ..] = parts.as_slice() else {
        return false;
    };
    velnor_actions_contract::config::is_valid_github_owner(owner)
        && velnor_actions_contract::config::is_valid_github_repo_name(repo)
        && matches!(
            rest,
            ["git", "tags" | "blobs" | "trees" | "commits" | "refs"] | ["releases" | "pulls"]
        )
}

pub(super) fn valid_route(route: &str) -> bool {
    if route.len() > 4096 || !route.is_ascii() || route.contains('#') {
        return false;
    }
    let (path, query) = route.split_once('?').map_or((route, ""), |(p, q)| (p, q));
    if query.contains('?') {
        return false;
    }
    let segments: Vec<&str> = path.split('/').collect();
    if let ["users", user] = segments.as_slice() {
        return query.is_empty() && valid_segment(user);
    }
    let ["repos", owner, repo, rest @ ..] = segments.as_slice() else {
        return false;
    };
    if !velnor_actions_contract::config::is_valid_github_owner(owner)
        || !velnor_actions_contract::config::is_valid_github_repo_name(repo)
    {
        return false;
    }
    match rest {
        [] => query.is_empty(),
        ["actions", "runs" | "artifacts" | "jobs", id]
        | ["actions", "runs", id, "approvals"]
        | ["rulesets" | "pulls", id] => numeric(id) && query.is_empty(),
        ["actions", "runs", id, "attempts", attempt] => {
            numeric(id) && numeric(attempt) && query.is_empty()
        }
        ["actions", "runs", id, "attempts", attempt, "jobs"] => {
            numeric(id) && numeric(attempt) && (query == "per_page=100" || page_query(query))
        }
        ["actions", "runs", id, "artifacts"] => {
            numeric(id) && (query == "per_page=100" || page_query(query))
        }
        ["actions", "runs"] => query
            .strip_prefix("head_sha=")
            .and_then(|q| q.split_once('&'))
            .is_some_and(|(sha, q)| valid_sha(sha) && page_query(q)),
        ["compare", range] => {
            range
                .split_once("...")
                .is_some_and(|(left, right)| valid_sha(left) && valid_segment(right))
                && query.is_empty()
        }
        ["environments" | "branches", name]
        | ["git", "ref", "heads" | "tags", name]
        | ["collaborators", name, "permission"]
        | ["releases", "tags", name] => valid_segment(name) && query.is_empty(),
        ["environments", name, "deployment-branch-policies"] => {
            valid_segment(name) && query == "per_page=100"
        }
        ["git", "matching-refs", "tags", prefix] => valid_segment(prefix) && query.is_empty(),
        ["git", "matching-refs", "heads", prefix] => {
            prefix.starts_with("release-plz-") && valid_segment(prefix) && query.is_empty()
        }
        ["git", "tags" | "commits" | "blobs" | "trees", sha] => valid_sha(sha) && query.is_empty(),
        ["pulls"] if query.starts_with("state=open&base=") => query
            .strip_prefix("state=open&base=")
            .and_then(|q| q.split_once('&'))
            .is_some_and(|(base, q)| valid_segment(base) && page_query(q)),
        ["pulls"] => query
            .strip_prefix("state=all&head=")
            .and_then(|rest| rest.split_once("&base="))
            .and_then(|(head, rest)| rest.strip_suffix("&per_page=100").map(|base| (head, base)))
            .is_some_and(|(head, base)| valid_segment(head) && valid_segment(base)),
        ["commits", sha, "pulls" | "comments"] => valid_sha(sha) && page_query(query),
        ["commits", sha, "check-runs"] => {
            valid_sha(sha) && query.strip_prefix("filter=all&").is_some_and(page_query)
        }
        ["pulls", id, "reviews" | "comments" | "commits"] | ["issues", id, "comments"] => {
            numeric(id) && page_query(query)
        }
        ["rulesets"] => query == "targets=tag&includes_parents=true&per_page=100",
        _ => false,
    }
}

pub(super) fn optional_route(route: &str) -> bool {
    if !valid_route(route) {
        return false;
    }
    let parts: Vec<&str> = route.split('/').collect();
    matches!(
        parts.as_slice(),
        ["repos", _, _, "git", "ref", "tags", _] | ["repos", _, _, "releases", "tags", _]
    )
}
