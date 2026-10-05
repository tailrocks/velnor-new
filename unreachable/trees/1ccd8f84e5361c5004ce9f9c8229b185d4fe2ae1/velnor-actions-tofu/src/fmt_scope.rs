//! Independent formatting scope (S2): inclusion, exclusion, per-root sets.
//!
//! Inclusion is by extension: `.tf`, `.tofu`, `.tfvars`,
//! `.tftest.hcl`, `.tofutest.hcl` (NOT JSON, NOT `.tofuvars`, NOT the
//! lockfile). Exclusion is the S2 predicate on the base name: starts
//! with `.`/`~`, or starts AND ends with `#`. Precedence never
//! narrows the fmt set: shadowed files still format. Hidden parent
//! directories (any segment starting with `.`) are skipped: cache
//! and VCS directories never format.

/// Suffixes in the fmt set, longest first.
const FMT_SUFFIXES: [&str; 5] = [".tofutest.hcl", ".tftest.hcl", ".tfvars", ".tofu", ".tf"];

/// True when `file_name` carries the S2 exclusion marks.
#[must_use]
pub fn is_excluded_name(file_name: &str) -> bool {
    file_name.starts_with('.')
        || file_name.starts_with('~')
        || (file_name.starts_with('#') && file_name.ends_with('#'))
}

/// True when `file_name` is fmt-included by extension and predicate.
#[must_use]
pub fn is_fmt_file(file_name: &str) -> bool {
    if is_excluded_name(file_name) {
        return false;
    }
    FMT_SUFFIXES.iter().any(|suffix| {
        file_name
            .strip_suffix(suffix)
            .is_some_and(|stem| !stem.is_empty())
    })
}

/// True when any parent segment of repo-relative `path` is hidden.
#[must_use]
pub fn under_hidden_dir(path: &str) -> bool {
    let mut segments: Vec<&str> = path.split('/').collect();
    segments.pop();
    segments.iter().any(|segment| segment.starts_with('.'))
}

/// Sorted fmt set among repo-relative `paths`.
#[must_use]
pub fn fmt_set(paths: &[String]) -> Vec<String> {
    let mut selected: Vec<String> = paths
        .iter()
        .filter(|path| {
            let name = path.rsplit('/').next().unwrap_or(path);
            is_fmt_file(name) && !under_hidden_dir(path)
        })
        .cloned()
        .collect();
    selected.sort();
    selected
}

/// Sorted fmt scope under `prefix` (`""` = repository root, recursive).
///
/// One fmt invocation covers one non-overlapping scope; overlap
/// merging across roots belongs to job grouping (T18).
#[must_use]
pub fn fmt_scope_for_root(paths: &[String], prefix: &str) -> Vec<String> {
    fmt_set(paths)
        .into_iter()
        .filter(|path| prefix.is_empty() || path.starts_with(&format!("{prefix}/")))
        .collect()
}

/// Roots whose fmt scope another selected root already covers.
///
/// Scopes nest by prefix: `""` covers every root, `a` covers `a/b`
/// but never the sibling `a-sibling`. A covered root formats nothing
/// itself (its leg carries `no_targets`); the covering leg checks the
/// union recursively, so every file formats exactly once. Roots must
/// be normalized (`.` already folded to `""`).
#[must_use]
pub fn covered_fmt_roots(roots: &[String]) -> std::collections::BTreeSet<String> {
    let mut covered = std::collections::BTreeSet::new();
    for root in roots {
        let dominated = roots.iter().any(|other| {
            other != root && (other.is_empty() || root.starts_with(&format!("{other}/")))
        });
        if dominated {
            covered.insert(root.clone());
        }
    }
    covered
}
