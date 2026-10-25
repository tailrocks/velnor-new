use std::collections::BTreeSet;

use crate::paths::is_under;

#[must_use]
pub fn affected_roots(
    roots: &[String],
    shared: &[String],
    changed: &BTreeSet<String>,
) -> BTreeSet<String> {
    if changed.iter().any(|path| shared.contains(path)) {
        return roots.iter().cloned().collect();
    }
    roots
        .iter()
        .filter(|root| changed.iter().any(|path| is_under(path, root)))
        .cloned()
        .collect()
}
