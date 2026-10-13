//! Private local seals are never adopted from cache transport.
use super::*;
use std::io::Write as _;

const NAMESPACE: &str = "build-receipts/v4/local-grants";

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootIdentity {
    device: u64,
    inode: u64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Grant {
    version: u8,
    proof: ReceiptLineage,
    roots: Vec<RootIdentity>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SealedGrant {
    grant: Grant,
    seal: [u8; 32],
}

fn root(config: &Config) -> PathBuf {
    config.store_dir().join(NAMESPACE)
}

fn grant_path(config: &Config, roots: &WorkspaceRoots) -> Result<PathBuf> {
    let digest = CacheDigest::blake3(&mbx_cache_core::canonical_json(roots)?);
    Ok(root(config).join(format!("{}.json", digest.hash)))
}

#[cfg(unix)]
fn check_private(path: &Path, directory: bool) -> Result<()> {
    use std::os::unix::fs::MetadataExt as _;
    let metadata = std::fs::symlink_metadata(path)?;
    let probe = tempfile::tempdir()?;
    let uid = std::fs::metadata(probe.path())?.uid();
    if metadata.file_type().is_symlink()
        || metadata.is_dir() != directory
        || (!directory && !metadata.is_file())
        || metadata.uid() != uid
        || metadata.mode() & 0o077 != 0
    {
        bail!("native lineage capability namespace is not private owner storage");
    }
    Ok(())
}

#[cfg(not(unix))]
fn check_private(_path: &Path, _directory: bool) -> Result<()> {
    bail!("native lineage local owner capability is unavailable on this platform")
}

#[cfg(unix)]
fn identities(roots: &WorkspaceRoots) -> Result<Vec<RootIdentity>> {
    use std::os::unix::fs::MetadataExt as _;
    [
        &roots.workspace_root,
        &roots.cargo.target_dir,
        &roots.cargo.build_dir,
    ]
    .into_iter()
    .map(|path| {
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            bail!("native lineage root identity is unavailable");
        }
        Ok(RootIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    })
    .collect()
}

#[cfg(not(unix))]
fn identities(_roots: &WorkspaceRoots) -> Result<Vec<RootIdentity>> {
    bail!("native lineage root identity is unavailable on this platform")
}

fn key(config: &Config, candidate: Option<&[u8; 32]>) -> Result<Option<[u8; 32]>> {
    let directory = root(config);
    if !directory.try_exists()? {
        if candidate.is_none() {
            return Ok(None);
        }
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt as _;
            builder.mode(0o700);
        }
        builder.create(&directory)?;
    }
    check_private(&directory, true)?;
    let path = directory.join("owner-key");
    if !path.try_exists()? {
        let bytes =
            candidate.ok_or_else(|| eyre::eyre!("native lineage local owner key was lost"))?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(mut file) => {
                file.write_all(bytes)?;
                file.sync_all()?;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    check_private(&path, false)?;
    let bytes = std::fs::read(&path)?;
    Ok(Some(bytes.try_into().map_err(|_| {
        eyre::eyre!("invalid local owner seal key")
    })?))
}

pub(super) fn invalidate(config: &Config, roots: &WorkspaceRoots) -> Result<()> {
    let path = grant_path(config, roots)?;
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn load(config: &Config, roots: &WorkspaceRoots) -> Result<Option<ReceiptLineage>> {
    let Some(key) = key(config, None)? else {
        return Ok(None);
    };
    let path = grant_path(config, roots)?;
    if !path.try_exists()? {
        return Ok(None);
    }
    check_private(&path, false)?;
    let bytes = std::fs::read(&path)?;
    let record: SealedGrant = serde_json::from_slice(&bytes)?;
    let actual =
        mbx_cache_core::local_owner_seal(&key, &mbx_cache_core::canonical_json(&record.grant)?);
    let mismatch = actual
        .iter()
        .zip(record.seal)
        .fold(0u8, |value, (left, right)| value | (left ^ right));
    if mismatch != 0
        || mbx_cache_core::canonical_json(&record)? != bytes
        || record.grant.version != 1
    {
        bail!("native lineage grant has invalid local seal or canonical encoding");
    }
    let proof = record.grant.proof;
    proof.validate()?;
    require_budget(config, &proof, bytes.len())?;
    if &proof.destination != roots
        || physical_roots(roots)? != proof.physical
        || identities(&proof.physical)? != record.grant.roots
    {
        bail!("native lineage destination roots or directory identities changed");
    }
    Ok(Some(proof))
}

pub(super) fn store(config: &Config, proof: &ReceiptLineage) -> Result<()> {
    // Compute the exact candidate record before any private namespace/key write.
    let candidate = if root(config).join("owner-key").try_exists()? {
        key(config, None)?.ok_or_else(|| eyre::eyre!("native lineage owner seal unavailable"))?
    } else {
        rand::random()
    };
    let grant = Grant {
        version: 1,
        proof: proof.clone(),
        roots: identities(&proof.physical)?,
    };
    let seal =
        mbx_cache_core::local_owner_seal(&candidate, &mbx_cache_core::canonical_json(&grant)?);
    let mut sealed = SealedGrant { grant, seal };
    let mut bytes = mbx_cache_core::canonical_json(&sealed)?;
    require_budget(config, proof, bytes.len())?;
    let persisted = key(config, Some(&candidate))?
        .ok_or_else(|| eyre::eyre!("native lineage owner seal unavailable"))?;
    if persisted != candidate {
        // A concurrent native issuer won key creation. Verify its actual seal
        // encoding also fits before writing this grant.
        sealed.seal = mbx_cache_core::local_owner_seal(
            &persisted,
            &mbx_cache_core::canonical_json(&sealed.grant)?,
        );
        bytes = mbx_cache_core::canonical_json(&sealed)?;
        require_budget(config, proof, bytes.len())?;
    }
    let mut staged = tempfile::NamedTempFile::new_in(root(config))?;
    staged.write_all(&bytes)?;
    staged.as_file().sync_all()?;
    staged.persist(grant_path(config, &proof.destination)?)?;
    Ok(())
}

fn require_budget(config: &Config, proof: &ReceiptLineage, grant_bytes: usize) -> Result<()> {
    let mut required = u64::try_from(grant_bytes)?
        .checked_add(32)
        .ok_or_else(|| eyre::eyre!("native lineage proof size overflow"))?;
    for object in &proof.selected_objects {
        required = required
            .checked_add(object.size)
            .ok_or_else(|| eyre::eyre!("native lineage closure size overflow"))?;
    }
    if required > config.gc.max_bytes {
        bail!(
            "native lineage required proof closure {required} bytes exceeds cache budget {}",
            config.gc.max_bytes
        );
    }
    Ok(())
}
