//! Expected-repository resolution: immutable CI env vs mutable origin.
//!
//! Declared via `#[path]` from `cover_baseline.rs` (no `lib.rs` edit).
//! Split from `provenance_check.rs` so both files keep the size gate.

use velnor_actions_contract::digest_b3;

use super::provenance_check::repository_slug_from_origin;

// Unit tests live here so `provenance_resolve.rs` keeps its size gate.
#[cfg(test)]
#[path = "provenance_resolve_tests.rs"]
mod provenance_resolve_tests;

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
