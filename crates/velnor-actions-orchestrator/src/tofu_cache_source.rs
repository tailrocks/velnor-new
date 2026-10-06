//! Provider transport admits only lock entries from public registries.
//!
//! Registry addresses describe publication scope, not provider trust. Hashes
//! remain enforced by readonly init; this proof only excludes private packages
//! from a cache readable by fork pull requests.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use velnor_actions_contract::ProposedTask;
use velnor_actions_tofu::FileCache;

/// Environment identity carrying the generation-captured provider export.
pub(crate) const PROVIDER_EXPORT_ENV: &str = crate::select_tofu::TOFU_PROVIDER_EXPORT;

/// Maximum lock bytes carried through task metadata and the export script.
const MAX_PROVIDER_EXPORT_LOCK_BYTES: usize = 16 * 1024;

/// Immutable provider evidence captured from one committed native lockfile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderExportDescriptor {
    /// Exact normalized root whose immutable lock authorizes this payload.
    pub(crate) root: String,
    /// Exact provider address/version pairs from the native lockfile.
    pub(crate) selections: Vec<(String, String)>,
    /// Exact UTF-8 lockfile bytes observed during generation.
    pub(crate) lock_content: String,
}

/// Recover one source-bound provider descriptor from identical qualified tasks.
///
/// Every task must carry the generation proof marker and a descriptor whose
/// selections still match its captured native lock. A modified lock payload,
/// unsafe version, private source, or disagreement between tasks disables the
/// export authority.
pub(crate) fn descriptor_for_tasks(tasks: &[&ProposedTask]) -> Option<ProviderExportDescriptor> {
    let first = tasks.first().copied()?;
    let descriptor = descriptor_from_task(first)?;
    if tasks
        .iter()
        .skip(1)
        .any(|task| descriptor_from_task(task) != Some(descriptor.clone()))
    {
        return None;
    }
    Some(descriptor)
}

/// Capture a bounded public provider descriptor for one selected root.
pub(crate) fn provider_export_descriptor_at_root(
    repository: &Path,
    root: &str,
    reads: &mut FileCache,
) -> Option<ProviderExportDescriptor> {
    // Keep this call as the existing source-origin gate. The descriptor adds
    // stronger version and byte checks after that established public proof.
    if !public_provider_sources_at_root(repository, root, reads) {
        return None;
    }
    let lock = contained_lock_path(repository, root)?;
    let bytes = reads.read_raw(&lock).ok()?;
    if bytes.len() > MAX_PROVIDER_EXPORT_LOCK_BYTES {
        return None;
    }
    let content = std::str::from_utf8(&bytes).ok()?;
    descriptor_from_lock(root, content)
}

/// Prove every locked provider uses an explicitly public registry hostname.
/// Missing, corrupt, empty, or unpinned selections cannot authorize transport.
pub(crate) fn public_provider_sources(content: Option<&str>) -> bool {
    let Ok(inspection) = velnor_actions_tofu::inspect_lockfile(".terraform.lock.hcl", content)
    else {
        return false;
    };
    let Some(spec) = inspection.spec else {
        return false;
    };
    inspection.findings.is_empty()
        && !spec.providers.is_empty()
        && spec.providers.iter().all(|source| public_address(source))
}

/// Read the root's contained, nonsymlink lock through the bounded file cache.
pub(crate) fn public_provider_sources_at_root(
    repository: &Path,
    root: &str,
    reads: &mut FileCache,
) -> bool {
    if velnor_actions_tofu::validate_normalized_root(root).is_err() {
        return false;
    }
    let Some(lock) = contained_lock_path(repository, root) else {
        return false;
    };
    let Ok(bytes) = reads.read_raw(&lock) else {
        return false;
    };
    let Ok(content) = std::str::from_utf8(&bytes) else {
        return false;
    };
    public_provider_sources(Some(content))
}

/// Return the exact root lock only when its resolved path stays in-repository.
fn contained_lock_path(repository: &Path, root: &str) -> Option<PathBuf> {
    let lock = repository.join(root).join(".terraform.lock.hcl");
    let canonical_repository = repository.canonicalize().ok()?;
    lock.canonicalize()
        .ok()
        .filter(|path| path.starts_with(canonical_repository))
}

/// Parse and validate the source-bound descriptor from immutable lock bytes.
pub(crate) fn descriptor_from_lock(root: &str, content: &str) -> Option<ProviderExportDescriptor> {
    if velnor_actions_tofu::root_for_key(&velnor_actions_tofu::key_for_root(root)).is_err()
        || content.len() > MAX_PROVIDER_EXPORT_LOCK_BYTES
        || !public_provider_sources(Some(content))
    {
        return None;
    }
    let inspection =
        velnor_actions_tofu::inspect_lockfile(".terraform.lock.hcl", Some(content)).ok()?;
    if !inspection.findings.is_empty() {
        return None;
    }
    let spec = inspection.spec?;
    if spec.providers.is_empty() {
        return None;
    }

    // Inspect the native model as well as LockfileSpec. This rejects a
    // duplicate provider block that has a hash but no literal version, which
    // the compact selection projection cannot represent by itself.
    let model = velnor_actions_tofu::parse_native(content).ok()?;
    if model.provider_hash_counts.is_empty()
        || model.provider_hash_counts.iter().any(|entry| {
            entry.hashes == 0
                || !public_address(&entry.address)
                || entry
                    .version
                    .as_deref()
                    .is_none_or(|version| !safe_version(version))
        })
    {
        return None;
    }

    let selections = canonical_selections(&spec.providers, &spec.selections)?;
    Some(ProviderExportDescriptor {
        root: root.to_owned(),
        selections,
        lock_content: content.to_owned(),
    })
}

/// Decode one task's descriptor and re-prove its captured lock bytes.
fn descriptor_from_task(task: &ProposedTask) -> Option<ProviderExportDescriptor> {
    if task.stack_id != velnor_actions_tofu::STACK_ID
        || task
            .identity
            .environment
            .get(crate::select_tofu::PUBLIC_PROVIDER_TRANSPORT)
            .map(String::as_str)
            != Some("true")
    {
        return None;
    }
    let value = task.identity.environment.get(PROVIDER_EXPORT_ENV)?;
    let descriptor: ProviderExportDescriptor = serde_json::from_str(value).ok()?;
    let root = velnor_actions_tofu::normalized_root_for_proposal(task).ok()?;
    if descriptor.root != root {
        return None;
    }
    let expected = descriptor_from_lock(root, &descriptor.lock_content)?;
    (expected.selections == descriptor.selections).then_some(descriptor)
}

/// Sort and validate exact provider selections against parsed addresses.
fn canonical_selections(
    providers: &[String],
    selections: &[(String, String)],
) -> Option<Vec<(String, String)>> {
    let mut by_address = BTreeMap::new();
    for (address, version) in selections {
        if !public_address(address) || !safe_version(version) {
            return None;
        }
        if by_address
            .insert(address.clone(), version.clone())
            .is_some_and(|previous| previous != *version)
        {
            return None;
        }
    }

    let mut expected = providers.to_vec();
    expected.sort();
    expected.dedup();
    if expected.len() != by_address.len()
        || expected
            .iter()
            .any(|address| !by_address.contains_key(address))
    {
        return None;
    }
    Some(by_address.into_iter().collect())
}

/// Accept only lowercase ASCII semver-like exact provider versions.
fn safe_version(version: &str) -> bool {
    if version.is_empty() || version.len() > 128 || !version.as_bytes()[0].is_ascii_digit() {
        return false;
    }
    if !version.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'+' | b'-')
    }) {
        return false;
    }
    let core_end = version.find(['+', '-']).unwrap_or(version.len());
    let core = &version[..core_end];
    let components: Vec<&str> = core.split('.').collect();
    components.len() == 3
        && components.iter().all(|component| {
            !component.is_empty() && component.bytes().all(|byte| byte.is_ascii_digit())
        })
        && version
            .get(core_end..)
            .is_none_or(|suffix| suffix.len() > 1)
}

fn public_address(source: &str) -> bool {
    let mut parts = source.split('/');
    let Some("registry.opentofu.org" | "registry.terraform.io") = parts.next() else {
        return false;
    };
    let (Some(namespace), Some(provider), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    [namespace, provider].iter().all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    })
}

#[cfg(test)]
#[path = "tofu_cache_source_tests.rs"]
mod tests;
