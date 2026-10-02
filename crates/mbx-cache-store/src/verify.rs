//! Read-only verification of an exact owner-exported directory bundle.
use super::*;

/// Owner-validated bundle state and a digest of every physical file and mode.
#[derive(Debug, Serialize)]
pub struct BundleVerification {
    pub version: u8,
    pub actions: u64,
    pub objects: u64,
    pub files: u64,
    pub bytes: u64,
    pub physical_digest: CacheDigest,
    pub comparison: ComparisonState,
    /// Explicit extra CAS roots declared by the owner export manifest.
    pub attachment_objects: BTreeSet<CacheDigest>,
}

#[derive(Serialize)]
struct PhysicalEntry {
    mode: u32,
    content: Option<CacheDigest>,
}

/// Verify without importing, consuming, executing, or hydrating the bundle.
pub fn verify_directory_bundle(root: &Path) -> Result<BundleVerification> {
    require_supported_host()?;
    let physical = physical_inventory(root)?;
    let (manifest, actions) = read_export_manifest(root)?;
    let mut closure = strict_closure(root, &actions, &manifest.action_owners)
        .wrap_err("cache export is incomplete or corrupt")?;
    let cas = LocalCas::new(root);
    for digest in &manifest.objects {
        require_object(&cas, &mut closure, digest)?;
    }
    verify_pending(&mut closure.pending)?;
    verify_exact_inventory(root, &physical, &closure)?;
    let comparison = ComparisonState::from_manifest(root, &manifest)?;
    comparison.validate()?;
    let physical_bytes = mbx_cache_core::canonical_json(&physical)?;
    if physical_bytes != mbx_cache_core::canonical_json(&physical_inventory(root)?)? {
        eyre::bail!("cache bundle changed during verification");
    }
    let files = physical
        .values()
        .filter(|entry| entry.content.is_some())
        .count() as u64;
    let bytes = physical.values().try_fold(0_u64, |total, entry| {
        total
            .checked_add(entry.content.as_ref().map_or(0, |digest| digest.size))
            .ok_or_else(|| eyre::eyre!("cache bundle byte count overflow"))
    })?;
    Ok(BundleVerification {
        version: 1,
        actions: actions.len() as u64,
        objects: closure.objects.len() as u64,
        files,
        bytes,
        physical_digest: CacheDigest::blake3(&physical_bytes),
        comparison,
        attachment_objects: manifest.objects.into_iter().collect(),
    })
}

#[cfg(unix)]
fn require_supported_host() -> Result<()> {
    Ok(())
}

#[cfg(not(unix))]
fn require_supported_host() -> Result<()> {
    eyre::bail!(
        "strict cache verification is unsupported on this host: native hard-link inspection is unavailable"
    )
}

fn physical_inventory(root: &Path) -> Result<BTreeMap<String, PhysicalEntry>> {
    let metadata = std::fs::symlink_metadata(root)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        eyre::bail!("cache verification requires a plain directory bundle");
    }
    let mut inventory = BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            let metadata = std::fs::symlink_metadata(&path)?;
            let relative = path.strip_prefix(root)?;
            let name = relative
                .to_str()
                .ok_or_else(|| eyre::eyre!("non-UTF-8 bundle path"))?
                .replace(std::path::MAIN_SEPARATOR, "/");
            let content = if metadata.is_dir() {
                pending.push(path);
                None
            } else if metadata.is_file() {
                validate_archive_path(relative)?;
                reject_linked_file(&metadata, &path)?;
                Some(CacheDigest::blake3_file(&path)?)
            } else {
                eyre::bail!(
                    "cache export contains a non-file entry at {}",
                    path.display()
                );
            };
            inventory.insert(
                name,
                PhysicalEntry {
                    mode: physical_mode(&metadata),
                    content,
                },
            );
        }
    }
    Ok(inventory)
}

fn verify_exact_inventory(
    root: &Path,
    physical: &BTreeMap<String, PhysicalEntry>,
    closure: &Closure,
) -> Result<()> {
    let mut expected = BTreeSet::from([EXPORT_MANIFEST.to_owned()]);
    for path in closure.objects.iter().chain(&closure.results) {
        let relative = path.strip_prefix(root)?;
        expected.insert(
            relative
                .to_str()
                .ok_or_else(|| eyre::eyre!("non-UTF-8 bundle path"))?
                .replace(std::path::MAIN_SEPARATOR, "/"),
        );
        for parent in relative.ancestors().skip(1) {
            if !parent.as_os_str().is_empty() {
                expected.insert(
                    parent
                        .to_str()
                        .ok_or_else(|| eyre::eyre!("non-UTF-8 bundle path"))?
                        .replace(std::path::MAIN_SEPARATOR, "/"),
                );
            }
        }
    }
    if expected != physical.keys().cloned().collect() {
        eyre::bail!("cache export physical inventory differs from its validated closure");
    }
    Ok(())
}

#[cfg(unix)]
fn physical_mode(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::MetadataExt as _;
    metadata.mode()
}

#[cfg(not(unix))]
fn physical_mode(metadata: &std::fs::Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

#[cfg(all(test, unix))]
#[path = "verify_tests.rs"]
mod tests;

#[cfg(all(test, not(unix)))]
#[test]
fn rejects_hosts_without_safe_hard_link_inspection() {
    let error = verify_directory_bundle(Path::new("missing-bundle")).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("hard-link inspection is unavailable")
    );
}
