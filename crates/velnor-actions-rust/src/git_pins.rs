//! Git dependency pin check: private sources must be pinned to a tag.
//!
//! A `git` dependency or `[patch]` on a repository of the project's own
//! GitHub owner is a private cross-repository source. Cargo resolves it to
//! `git+<url>[?tag=...|?branch=...|?rev=...]#<sha>`; only `?tag=` is a stable,
//! reviewable pin (a branch moves, and a bare revision hides the release it
//! belongs to). The lock's `#<sha>` always carries the exact commit.

use serde::Deserialize;

/// One private git source that is not pinned to a tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnpinnedGitSource {
    /// Resolved package name.
    pub package: String,
    /// Source URL without the commit fragment.
    pub source: String,
}

/// Minimal view of `cargo metadata`.
#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}

/// Minimal view of one resolved package.
#[derive(Deserialize)]
struct Package {
    name: String,
    source: Option<String>,
}

/// List git sources on `owner`'s GitHub repositories lacking a `tag` pin.
///
/// # Errors
///
/// Returns the JSON error when the metadata document is malformed.
pub fn unpinned_git_sources(
    metadata_json: &str,
    owner: &str,
) -> Result<Vec<UnpinnedGitSource>, serde_json::Error> {
    let metadata: Metadata = serde_json::from_str(metadata_json)?;
    let mut found: Vec<UnpinnedGitSource> = metadata
        .packages
        .into_iter()
        .filter_map(|package| {
            let source = package.source?;
            let url = source.strip_prefix("git+")?;
            let url = url.split('#').next().unwrap_or(url);
            (is_owner_url(url, owner) && !has_tag_pin(url)).then(|| UnpinnedGitSource {
                package: package.name,
                source: url.to_owned(),
            })
        })
        .collect();
    found.sort_by(|a, b| (&a.package, &a.source).cmp(&(&b.package, &b.source)));
    found.dedup();
    Ok(found)
}

/// True for `https://github.com/<owner>/...` or `ssh://git@github.com/<owner>/...`.
fn is_owner_url(url: &str, owner: &str) -> bool {
    let path = [
        "https://github.com/",
        "ssh://git@github.com/",
        "git://github.com/",
    ]
    .iter()
    .find_map(|prefix| url.strip_prefix(prefix));
    path.and_then(|path| path.split('/').next())
        .is_some_and(|segment| segment.eq_ignore_ascii_case(owner))
}

/// True when the query string carries exactly one non-empty `tag=` pin.
fn has_tag_pin(url: &str) -> bool {
    let Some((_, query)) = url.split_once('?') else {
        return false;
    };
    let mut tags = query
        .split('&')
        .filter_map(|pair| pair.strip_prefix("tag="));
    let tag = tags.next();
    tag.is_some_and(|tag| !tag.is_empty()) && tags.next().is_none()
}

#[cfg(test)]
#[path = "git_pins_tests.rs"]
mod tests;
