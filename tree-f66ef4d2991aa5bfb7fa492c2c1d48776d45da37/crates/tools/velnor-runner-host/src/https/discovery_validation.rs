use super::Origin;
use velnor_runner_github::{Method, SessionRequest};

#[path = "discovery_actions_delete_validation.rs"]
mod actions_delete_validation;

pub(super) struct ValidatedTarget<'a> {
    pub(super) path: &'a str,
    pub(super) query: Option<&'a str>,
}

fn validated_target<'a>(
    path: &'a str,
    query: Option<&'a str>,
    allowed: bool,
) -> Option<ValidatedTarget<'a>> {
    allowed.then_some(ValidatedTarget { path, query })
}

pub(super) fn validate_discovery_request<'a>(
    origin: &Origin,
    request: &'a SessionRequest,
) -> Option<ValidatedTarget<'a>> {
    let path = request.path.strip_prefix('/').unwrap_or(&request.path);
    if path.starts_with('/') || path.contains(['\\', '%', '#', '?']) {
        return None;
    }
    match origin {
        Origin::GithubApi => validate_github_request(path, request),
        Origin::Actions(_) => validate_actions_request(path, request),
    }
}

fn validate_github_request<'a>(
    path: &'a str,
    request: &'a SessionRequest,
) -> Option<ValidatedTarget<'a>> {
    if !request.body.is_empty() && request.method == Method::Get {
        return None;
    }
    match request.method {
        Method::Get if request.query.is_none() && repository_path(path) => {
            headers_are(request, GithubHeaders::RepositoryGet)
                .then_some(ValidatedTarget { path, query: None })
        }
        Method::Get if request.query.is_none() && workflow_run_path(path) => {
            headers_are(request, GithubHeaders::RepositoryGet)
                .then_some(ValidatedTarget { path, query: None })
        }
        Method::Get
            if attempt_jobs_path(path)
                && request
                    .query
                    .as_deref()
                    .is_some_and(valid_attempt_jobs_query) =>
        {
            headers_are(request, GithubHeaders::RepositoryGet).then_some(ValidatedTarget {
                path,
                query: request.query.as_deref(),
            })
        }
        Method::Post
            if request.query.is_none() && registration_path(path) && request.body.is_empty() =>
        {
            headers_are(request, GithubHeaders::RegistrationToken)
                .then_some(ValidatedTarget { path, query: None })
        }
        Method::Post
            if request.query.is_none()
                && admin_exchange_path(path)
                && valid_admin_body(path, &request.body) =>
        {
            headers_are(request, GithubHeaders::AdminExchange)
                .then_some(ValidatedTarget { path, query: None })
        }
        _ => None,
    }
}

fn validate_actions_request<'a>(
    path: &'a str,
    request: &'a SessionRequest,
) -> Option<ValidatedTarget<'a>> {
    if !request.body.is_empty() {
        return None;
    }
    let admin_headers = ActionsHeaders::AdminJsonBearer;
    match (request.method, path, request.query.as_deref()) {
        (Method::Get, "_apis/runtime/runnergroups", Some("api-version=6.0-preview")) => {
            validated_target(
                path,
                request.query.as_deref(),
                headers_are(request, admin_headers),
            )
        }
        (Method::Get, "_apis/runtime/runnerscalesets", Some(query)) => validated_target(
            path,
            request.query.as_deref(),
            headers_are(request, admin_headers)
                && actions_delete_validation::valid_scale_set_query(query),
        ),
        (Method::Delete, path, Some(query)) => validated_target(
            path,
            request.query.as_deref(),
            query == "api-version=6.0-preview"
                && actions_delete_validation::session_delete_path(path)
                && headers_are(request, admin_headers),
        ),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum GithubHeaders {
    RepositoryGet,
    RegistrationToken,
    AdminExchange,
}

#[derive(Clone, Copy)]
enum ActionsHeaders {
    AdminJsonBearer,
}

fn headers_are(request: &SessionRequest, kind: impl HeaderKind) -> bool {
    let mut authorization = None;
    let mut content_type = None;
    let mut accept = None;
    let mut api_version = None;
    let mut user_agent = None;
    for (name, value) in &request.headers {
        if value.bytes().any(|byte| byte.is_ascii_control()) {
            return false;
        }
        let slot = if name.eq_ignore_ascii_case("authorization") {
            &mut authorization
        } else if name.eq_ignore_ascii_case("content-type") {
            &mut content_type
        } else if name.eq_ignore_ascii_case("accept") {
            &mut accept
        } else if name.eq_ignore_ascii_case("x-github-api-version") {
            &mut api_version
        } else if name.eq_ignore_ascii_case("user-agent") {
            &mut user_agent
        } else {
            return false;
        };
        if slot.replace(value.as_str()).is_some() {
            return false;
        }
    }
    kind.matches(authorization, content_type, accept, api_version, user_agent)
}

trait HeaderKind {
    fn matches(
        self,
        authorization: Option<&str>,
        content_type: Option<&str>,
        accept: Option<&str>,
        api_version: Option<&str>,
        user_agent: Option<&str>,
    ) -> bool;
}

impl HeaderKind for GithubHeaders {
    fn matches(
        self,
        auth: Option<&str>,
        content: Option<&str>,
        accept: Option<&str>,
        version: Option<&str>,
        agent: Option<&str>,
    ) -> bool {
        match self {
            Self::RepositoryGet => {
                auth_scheme(auth, "Bearer")
                    && content.is_none()
                    && accept == Some("application/vnd.github+json")
                    && version == Some("2026-03-10")
                    && agent == Some("velnor-host")
            }
            Self::RegistrationToken => {
                auth_scheme(auth, "Bearer")
                    && content == Some("application/vnd.github.v3+json")
                    && accept.is_none()
                    && version.is_none()
                    && agent == Some("velnor-host")
            }
            Self::AdminExchange => {
                auth_scheme(auth, "RemoteAuth")
                    && content == Some("application/json")
                    && accept.is_none()
                    && version.is_none()
                    && agent == Some("velnor-host")
            }
        }
    }
}

impl HeaderKind for ActionsHeaders {
    fn matches(
        self,
        auth: Option<&str>,
        content: Option<&str>,
        accept: Option<&str>,
        version: Option<&str>,
        agent: Option<&str>,
    ) -> bool {
        match self {
            Self::AdminJsonBearer => {
                auth_scheme(auth, "Bearer")
                    && content == Some("application/json")
                    && accept.is_none()
                    && version.is_none()
                    && agent == Some("velnor-host")
            }
        }
    }
}

fn auth_scheme(value: Option<&str>, scheme: &str) -> bool {
    let Some(value) = value.and_then(|value| {
        value
            .strip_prefix(scheme)
            .and_then(|tail| tail.strip_prefix(' '))
    }) else {
        return false;
    };
    !value.is_empty()
        && value.len() <= 8 * 1024
        && value.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
}

fn repository_path(path: &str) -> bool {
    let mut parts = path.split('/');
    matches!(parts.next(), Some("repos"))
        && parts.next().is_some_and(path_segment)
        && parts.next().is_some_and(path_segment)
        && parts.next().is_none()
}

fn workflow_run_path(path: &str) -> bool {
    let mut parts = path.split('/');
    matches!(parts.next(), Some("repos"))
        && parts.next().is_some_and(path_segment)
        && parts.next().is_some_and(path_segment)
        && matches!(parts.next(), Some("actions"))
        && matches!(parts.next(), Some("runs"))
        && parts.next().is_some_and(positive_canonical_u64)
        && parts.next().is_none()
}

fn attempt_jobs_path(path: &str) -> bool {
    let mut parts = path.split('/');
    matches!(parts.next(), Some("repos"))
        && parts.next().is_some_and(path_segment)
        && parts.next().is_some_and(path_segment)
        && matches!(parts.next(), Some("actions"))
        && matches!(parts.next(), Some("runs"))
        && parts.next().is_some_and(positive_canonical_u64)
        && matches!(parts.next(), Some("attempts"))
        && parts.next().is_some_and(positive_canonical_u64)
        && matches!(parts.next(), Some("jobs"))
        && parts.next().is_none()
}

fn positive_canonical_u64(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && value.bytes().all(|byte| byte.is_ascii_digit())
        && value
            .parse::<u64>()
            .is_ok_and(|number| number > 0 && number.to_string() == value)
}

fn valid_attempt_jobs_query(query: &str) -> bool {
    let Some(page) = query.strip_prefix("per_page=100&page=") else {
        return false;
    };
    positive_canonical_u64(page)
        && page
            .parse::<u8>()
            .is_ok_and(|number| (1..=4).contains(&number))
}

fn registration_path(path: &str) -> bool {
    let mut parts = path.split('/');
    matches!(parts.next(), Some("repos"))
        && parts.next().is_some_and(path_segment)
        && parts.next().is_some_and(path_segment)
        && matches!(parts.next(), Some("actions"))
        && matches!(parts.next(), Some("runners"))
        && matches!(parts.next(), Some("registration-token"))
        && parts.next().is_none()
}

fn admin_exchange_path(path: &str) -> bool {
    path == "actions/runner-registration"
}

fn path_segment(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_admin_body(_path: &str, body: &[u8]) -> bool {
    const PREFIX: &[u8] = b"{\"url\":\"https://github.com/";
    const SUFFIX: &[u8] = b"\",\"runner_event\":\"register\"}";
    let Some(repo) = body
        .strip_prefix(PREFIX)
        .and_then(|body| body.strip_suffix(SUFFIX))
    else {
        return false;
    };
    let Ok(repo) = std::str::from_utf8(repo) else {
        return false;
    };
    let mut parts = repo.split('/');
    parts.next().is_some_and(path_segment)
        && parts.next().is_some_and(path_segment)
        && parts.next().is_none()
        && body.len() <= 2048
}

pub(super) fn actions_base(value: &str) -> Option<String> {
    if value.len() > 1024
        || value.bytes().any(|byte| {
            byte.is_ascii_whitespace()
                || byte.is_ascii_control()
                || matches!(byte, b'\\' | b'%' | b'?' | b'#' | b'@')
        })
    {
        return None;
    }
    let rest = value.strip_prefix("https://")?;
    let (authority, raw_path) = rest.split_once('/').unwrap_or((rest, ""));
    if authority.is_empty() || authority.contains('@') {
        return None;
    }
    let host = if let Some((host, port)) = authority.rsplit_once(':') {
        if port != "443" || host.contains(':') {
            return None;
        }
        host
    } else {
        authority
    };
    let region = host
        .strip_prefix("pipelinesghubeus")?
        .strip_suffix(".actions.githubusercontent.com")?;
    if region.is_empty()
        || region.len() > 3
        || !region.bytes().all(|byte| byte.is_ascii_digit())
        || region
            .parse::<u16>()
            .ok()
            .is_none_or(|value| value == 0 || value > 999)
    {
        return None;
    }
    let path = raw_path.strip_suffix('/').unwrap_or(raw_path);
    let has_invalid_path = if raw_path.is_empty() {
        false
    } else {
        raw_path.starts_with('/')
            || raw_path.contains("//")
            || path.split('/').count() > 6
            || path.split('/').any(|segment| {
                segment.is_empty()
                    || segment == "."
                    || segment == ".."
                    || !segment.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                    })
            })
    };
    if has_invalid_path {
        return None;
    }
    let prefix = format!("https://{host}");
    Some(if path.is_empty() {
        prefix
    } else {
        format!("{prefix}/{path}")
    })
}
