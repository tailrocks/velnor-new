//! Configured-roots qualification: grammar, containment, effective config.
//!
//! The caller passes the canonical checkout root plus the post-exclusion
//! index. Lexical containment is proven by the config grammar (no
//! absolutes, no `..`); this module adds canonical containment
//! (symlink-resolved, in-repo) and the per-root effective-config rule.
//! It returns one candidate per root; the caller suppresses emission
//! when the stack is ignored. The index records canonical-side paths,
//! so a symlink alias to an in-repo dir holds no indexed files and
//! fails `no_effective_config`: configure the real directory.

use std::path::Path;

use velnor_actions_contract::{ContractError, FileIndex, StackCandidate, TofuStackConfig};

use crate::effective::dir_has_effective_config;

/// Qualify every configured root against the checkout at `root`.
///
/// # Errors
///
/// Returns key-path config errors for lexical violations, unreadable
/// or escaping roots, and roots without effective configuration.
pub fn qualify_roots(
    file: &str,
    root: &Path,
    config: &TofuStackConfig,
    index: &FileIndex,
) -> Result<Vec<StackCandidate>, ContractError> {
    config.validate(file)?;
    let canonical = root
        .canonicalize()
        .map_err(|err| bad(file, format!("unreadable_repo_root:{}", err.kind())))?;
    let mut candidates = Vec::with_capacity(config.roots.len());
    for entry in &config.roots {
        let prefix = entry.unit_prefix();
        qualify_one(file, &canonical, entry.as_str(), prefix, index)?;
        candidates.push(StackCandidate {
            stack_id: crate::STACK_ID.to_owned(),
            unit_root: prefix.to_owned(),
        });
    }
    Ok(candidates)
}

/// Qualify one root: canonical containment plus effective config.
fn qualify_one(
    file: &str,
    canonical: &Path,
    display: &str,
    prefix: &str,
    index: &FileIndex,
) -> Result<(), ContractError> {
    let dir = canonical.join(prefix);
    let resolved = dir
        .canonicalize()
        .map_err(|_| bad(file, format!("unreadable_root:{display}")))?;
    if !resolved.starts_with(canonical) {
        return Err(bad(file, format!("symlink_escape:{display}")));
    }
    if !dir_has_effective_config(index.files(), prefix) {
        return Err(bad(file, format!("no_effective_config:{display}")));
    }
    Ok(())
}

/// One `stacks.tofu.roots` key-path error.
fn bad(file: &str, problem: String) -> ContractError {
    ContractError::config(file, "stacks.tofu.roots", problem)
}
