//! Pending owner use prevents optional persistence failures from certifying a snapshot.
use super::*;

const DIRECTORY: &str = ".pending-v1";
static CURRENT: Mutex<Option<PathBuf>> = Mutex::new(None);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pending {
    version: u8,
    workspace_root: PathBuf,
    cargo: CargoBuildRoots,
    source: PathBuf,
    digest: CacheDigest,
    context: mbx_cache_store::ReceiptContext,
}

fn directory(root: &Path, workspace: &WorkspaceRoots) -> Result<PathBuf> {
    source_path(
        root,
        &PathBuf::from(DIRECTORY).join(journal::key(workspace)?),
    )
}

pub(super) fn begin(real: &Path, root: &Path, stable: &Path) -> Result<()> {
    let context = journal::context()?;
    let workspace = journal::workspace()?;
    let source = real.strip_prefix(&workspace.cargo.build_dir)?.to_path_buf();
    relative(&source)?;
    let (manifest, _, _) = describe(stable)?;
    let digest = CacheDigest::blake3(&manifest);
    let directory = directory(root, &workspace)?;
    let mut current = CURRENT.lock().unwrap();
    if current.is_some() {
        bail!("generated output ownership is already pending");
    }
    std::fs::create_dir_all(&directory)?;
    let path = directory.join(format!(
        "{}-{}.json",
        std::process::id(),
        crate::util::random_string(12)
    ));
    let pending = Pending {
        version: 1,
        workspace_root: workspace.workspace_root,
        cargo: workspace.cargo,
        source,
        digest,
        context,
    };
    let temporary = tempfile::NamedTempFile::new_in(directory)?;
    std::fs::write(temporary.path(), mbx_cache_core::canonical_json(&pending)?)?;
    temporary.persist(&path)?;
    *current = Some(path);
    Ok(())
}

pub(super) fn finish() -> Result<()> {
    let mut current = CURRENT.lock().unwrap();
    if let Some(path) = current.as_ref() {
        std::fs::remove_file(path)?;
        *current = None;
    }
    Ok(())
}

pub(super) fn check(root: &Path, workspace: &WorkspaceRoots) -> Result<()> {
    let directory = directory(root, workspace)?;
    if !directory.try_exists()? {
        return Ok(());
    }
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if entry
            .path()
            .extension()
            .is_none_or(|extension| extension != "json")
        {
            continue;
        }
        if !entry.file_type()?.is_file() {
            bail!("pending generated output record is not a regular file");
        }
        let bytes = std::fs::read(entry.path())?;
        let pending: Pending = serde_json::from_slice(&bytes)?;
        if pending.version != 1
            || pending.workspace_root != workspace.workspace_root
            || pending.cargo != workspace.cargo
            || mbx_cache_core::canonical_json(&pending)? != bytes
        {
            bail!("invalid pending generated output owner record");
        }
        relative(&pending.source)?;
        pending.digest.validate()?;
        pending.context.validate()?;
        // A different context cannot prove this unit's outputs disappeared
        // from opaque scheduler state. Only the producing shim retires it.
        return Err(
            OwnerUnavailable("generated output ownership finalization is incomplete").into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_owner_use_cannot_be_inferred_away_by_empty_receipts() -> Result<()> {
        let temporary = tempfile::tempdir()?;
        let workspace = WorkspaceRoots {
            workspace_root: temporary.path().join("workspace"),
            cargo: CargoBuildRoots {
                target_dir: temporary.path().join("target"),
                build_dir: temporary.path().join("build"),
            },
        };
        let root = temporary.path().join("owner-root");
        let path = directory(&root, &workspace)?;
        std::fs::create_dir_all(&path)?;
        let pending = Pending {
            version: 1,
            workspace_root: workspace.workspace_root.clone(),
            cargo: workspace.cargo.clone(),
            source: "unit/out".into(),
            digest: CacheDigest::blake3(b"manifest"),
            context: mbx_cache_store::ReceiptContext {
                schema: 1,
                source: serde_json::json!({"fixture":"earlier-source"}),
                tool: serde_json::json!({"fixture":"earlier-tool"}),
            },
        };
        std::fs::write(
            path.join("attempt.json"),
            mbx_cache_core::canonical_json(&pending)?,
        )?;
        let error = check(&root, &workspace).expect_err("unfinished ownership must be unavailable");
        assert!(is_unavailable(&error));
        Ok(())
    }
}
