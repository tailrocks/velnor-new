//! Expected-repository resolution: request slug vs mutable origin.
//!
//! Declared via `#[path]` from `cover_baseline.rs` (no `lib.rs` edit).
//! Split from `provenance_check.rs` so both files keep the size gate.

use velnor_actions_contract::digest_b3;

// Unit tests live here so `provenance_resolve.rs` keeps its size gate.
#[cfg(test)]
mod tests;

/// Expected repository resolution: the request slug wins over the
/// mutable git origin; disagreement fails closed at the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ExpectedRepository {
    /// Lowercase `owner/repo` slug, request-first then origin fallback.
    pub(crate) slug: Option<String>,
    /// True when the request slug disagrees with the git origin, or
    /// the request slug is malformed (no trusted slug then).
    pub(crate) conflict: bool,
}

/// Resolve the expected repository slug from an origin slug and request.
///
/// A well-formed request slug always wins; the origin slug is the
/// local-run fallback when no request slug is set. A well-formed
/// request slug that disagrees with the origin is a conflict (a step
/// may have rewritten the origin): the request slug still resolves,
/// but the caller must fail closed on the flag. A *malformed*
/// request slug is also a conflict with no trusted slug: falling back
/// to the origin would let a prior step launder an evil origin behind
/// mangled request text, so callers fail closed instead of trusting
/// either side.
pub(crate) fn resolve_expected_repository(
    origin_slug: Option<&str>,
    request_slug: Option<&str>,
) -> ExpectedRepository {
    let Some(raw) = request_slug else {
        return ExpectedRepository {
            slug: origin_slug.map(str::to_owned),
            conflict: false,
        };
    };
    let Some(request) = crate::origin::validate_repository_slug(raw) else {
        return ExpectedRepository {
            slug: None,
            conflict: true,
        };
    };
    let conflict = origin_slug.is_some_and(|origin| origin != request);
    ExpectedRepository {
        slug: Some(request),
        conflict,
    }
}

/// Repository identity digest for one expected slug.
pub(crate) fn repository_anchor_for_slug(slug: &str) -> String {
    digest_b3(format!("github.com/{slug}").as_bytes())
}
