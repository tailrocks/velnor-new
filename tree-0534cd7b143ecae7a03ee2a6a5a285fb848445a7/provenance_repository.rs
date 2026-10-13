//! Expected-repository resolution: immutable CI env plus git origin.
//!
//! Declared via `#[path]` from `provenance_check.rs` (size gate).

use velnor_actions_contract::digest_b3;

/// Expected repository resolution: immutable CI env wins over the
/// mutable git origin; disagreement fails closed at the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExpectedRepository {
    /// Lowercase `owner/repo` slug, env-first then origin fallback.
    pub(crate) slug: Option<String>,
    /// True when a well-formed env slug disagrees with the git origin.
    pub(crate) conflict: bool,
}

/// Resolve the expected repository slug from an origin slug and raw env.
///
/// A well-formed env slug always wins; the origin slug is the
/// local-run fallback when no env slug is set. A well-formed env slug
/// that disagrees with the origin is a conflict (a step may have
/// rewritten the origin): the env slug still resolves, but the caller
/// must fail closed on the flag. A *malformed* env slug is also a
/// conflict with no trusted slug: falling back to the origin would let
/// a prior step launder an evil origin behind mangled env text, so
/// callers fail closed instead of trusting either side.
pub(crate) fn resolve_expected_repository(
    origin_slug: Option<&str>,
    env_slug: Option<&str>,
) -> ExpectedRepository {
    let Some(raw) = env_slug else {
        return ExpectedRepository {
            slug: origin_slug.map(str::to_owned),
            conflict: false,
        };
    };
    let Some(env) = crate::origin::validate_repository_slug(raw) else {
        return ExpectedRepository {
            slug: None,
            conflict: true,
        };
    };
    let conflict = origin_slug.is_some_and(|origin| origin != env);
    ExpectedRepository {
        slug: Some(env),
        conflict,
    }
}

/// Expected repository for `root`: env slug plus origin fallback.
///
/// Reads [`GITHUB_REPOSITORY_ENV`](crate::origin::GITHUB_REPOSITORY_ENV)
/// and the git origin, then resolves through
/// [`resolve_expected_repository`].
pub(crate) fn expected_repository_for_root(root: &std::path::Path) -> ExpectedRepository {
    let env = std::env::var(crate::origin::GITHUB_REPOSITORY_ENV).ok();
    resolve_expected_repository(repository_slug_from_origin(root).as_deref(), env.as_deref())
}

/// Repository identity digest for one expected slug.
pub(crate) fn repository_anchor_for_slug(slug: &str) -> String {
    digest_b3(format!("github.com/{slug}").as_bytes())
}

/// Lowercase `owner/repo` slug from the git origin URL, when the origin
/// is a `github.com` remote a workflow slug can name.
///
/// Resolution runs through the shared [`crate::origin::origin_url_via_git`]
/// helper, so linked worktrees, includes, and worktree configuration all
/// follow Git semantics. Other hosts have no slug form comparable to
/// `owner/repo`; those checkouts fail closed in repository validation,
/// never warn-and-proceed.
pub(crate) fn repository_slug_from_origin(root: &std::path::Path) -> Option<String> {
    let url = crate::origin::origin_url_via_git(root)?;
    normalize_origin_url(&url).and_then(|normalized| {
        normalized
            .strip_prefix("github.com/")
            .map(str::to_owned)
            .filter(|slug| {
                let mut parts = slug.split('/');
                matches!(
                    (parts.next(), parts.next(), parts.next()),
                    (Some(owner), Some(repo), None)
                        if !owner.is_empty() && !repo.is_empty()
                )
            })
    })
}

/// Normalize an origin URL to `host/path` for identity comparison.
///
/// Accepts `https://` (and `http://`) plus scp-like `user@host:path`
/// forms; strips credentials, ports, and trailing `.git`; hosting
/// slugs compare case-insensitively.
pub(crate) fn normalize_origin_url(url: &str) -> Option<String> {
    let trimmed = url.trim().trim_end_matches('/');
    let trimmed = trimmed.strip_suffix(".git").unwrap_or(trimmed);
    if trimmed.is_empty() {
        return None;
    }
    let (hostport, path) = if let Some(rest) = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .or_else(|| trimmed.strip_prefix("ssh://"))
    {
        let rest = rest.rsplit_once('@').map_or(rest, |(_, rest)| rest);
        rest.split_once('/')?
    } else {
        let rest = trimmed.rsplit_once('@').map_or(trimmed, |(_, rest)| rest);
        rest.split_once(':')?
    };
    let host = hostport.split_once(':').map_or(hostport, |(host, _)| host);
    if host.is_empty() || path.is_empty() {
        return None;
    }
    Some(format!(
        "{}/{}",
        host.to_lowercase(),
        path.trim_matches('/').to_lowercase()
    ))
}
