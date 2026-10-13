//! Owner records close generated inputs over the native Cargo snapshot.
use super::*;
use eyre::bail;
use mbx_cache_store::{CargoBuildRoots, WorkspaceRoots};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::time::UNIX_EPOCH;

#[path = "out_dir_journal.rs"]
mod journal;
#[path = "out_dir_partition.rs"]
mod partition;
#[cfg(test)]
use journal::snapshot;
pub(crate) use journal::{capture, finalize, register};
#[path = "out_dir_pending.rs"]
mod pending;
pub(super) fn abandon() -> Result<()> {
    pending::finish()
}

#[path = "out_dir_roots.rs"]
mod roots;
pub(crate) use roots::resolve_root;

#[path = "out_dir_inventory.rs"]
mod inventory;
pub(crate) use inventory::{inventory_matches, validate_inventory};

#[path = "out_dir_evidence.rs"]
mod evidence;
pub(crate) use evidence::validate_receipts;

#[path = "out_dir_cas.rs"]
mod cas;
pub(crate) use cas::{hydrate_cas, validate_cas};

#[derive(Debug)]
struct OwnerUnavailable(&'static str);

impl std::fmt::Display for OwnerUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for OwnerUnavailable {}

pub(crate) fn is_unavailable(error: &eyre::Report) -> bool {
    error.downcast_ref::<OwnerUnavailable>().is_some()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Snapshot {
    pub source: PathBuf,
    pub root: PathBuf,
    pub digest: CacheDigest,
    pub directories: Vec<PathBuf>,
    pub files: Vec<FileTime>,
    pub proofs: Vec<Proof>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileTime {
    pub path: PathBuf,
    pub digest: CacheDigest,
    pub executable: bool,
    pub modified_secs: u64,
    pub modified_nanos: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Proof {
    pub identity: String,
    pub invocation: CacheDigest,
    pub action: CacheDigest,
    pub context: mbx_cache_store::ReceiptContext,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u8,
    workspace_root: PathBuf,
    cargo_roots: CargoBuildRoots,
    context: mbx_cache_store::ReceiptContext,
    snapshot: Snapshot,
}

pub(crate) fn validate_snapshot(snapshot: &Snapshot) -> Result<()> {
    relative(&snapshot.source)?;
    if !snapshot.root.is_absolute()
        || snapshot
            .root
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
        || snapshot.root.components().collect::<PathBuf>().as_os_str() != snapshot.root.as_os_str()
    {
        bail!("invalid generated output owner root");
    }
    snapshot.digest.validate()?;
    if snapshot.digest.algorithm != "blake3" {
        bail!("unsupported generated output owner digest");
    }
    let mut paths = BTreeSet::new();
    for file in &snapshot.files {
        relative(&file.path)?;
        file.digest.validate()?;
        if file.digest.algorithm != "blake3" {
            bail!("generated output file digest is not blake3");
        }
        if file.modified_nanos >= 1_000_000_000 || !paths.insert(&file.path) {
            bail!("invalid generated output timestamp metadata");
        }
        let _ = modified(file)?;
    }
    let mut directories = BTreeSet::new();
    for directory in &snapshot.directories {
        relative(directory)?;
        if !directories.insert(directory) {
            bail!("duplicate generated output directory");
        }
    }
    if snapshot.proofs.is_empty() {
        bail!("generated output has no successful action ownership proof");
    }
    let mut proofs = BTreeSet::new();
    for proof in &snapshot.proofs {
        proof.invocation.validate()?;
        proof.action.validate()?;
        proof.context.validate()?;
        if proof.identity.len() != 64
            || !proof.identity.bytes().all(|byte| byte.is_ascii_hexdigit())
            || !proofs.insert(evidence::proof_key(proof)?)
        {
            bail!("invalid generated output action ownership proof");
        }
    }
    inventory::validate_declared(snapshot)
}

pub(super) fn validate_live(real: &Path, root: &Path, stable: &Path) -> Result<()> {
    let (manifest, _, _) = describe(real)?;
    if stable != root.join(CacheDigest::blake3(&manifest).hash) {
        bail!("generated output view differs from original source");
    }
    let (view, files, directories) = describe(stable)?;
    if manifest != view {
        bail!("generated output view differs from original source");
    }
    for (file, executable) in files {
        protected(&stable.join(file), executable)?;
    }
    for directory in directories {
        protected(&stable.join(directory), true)?;
    }
    protected(stable, true)?;
    pending::begin(real, root, stable)
}

pub(crate) fn source_path(root: &Path, relative_path: &Path) -> Result<PathBuf> {
    relative(relative_path)?;
    let mut source = root.to_path_buf();
    for component in relative_path.components() {
        source.push(component);
        if std::fs::symlink_metadata(&source)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            bail!("generated output source crosses a symlink");
        }
    }
    Ok(source)
}

fn relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        || path.components().collect::<PathBuf>().as_os_str() != path.as_os_str()
    {
        bail!("invalid generated output relative path");
    }
    for component in path.components() {
        let name = component
            .as_os_str()
            .to_str()
            .ok_or_else(|| eyre::eyre!("generated output path is not UTF-8"))?;
        if name.contains('\\') || name.contains('\n') || name.contains('\0') {
            bail!("ambiguous generated output path");
        }
    }
    Ok(())
}

fn describe(source: &Path) -> Result<(Vec<u8>, Vec<(PathBuf, bool)>, Vec<PathBuf>)> {
    if !std::fs::symlink_metadata(source)?.file_type().is_dir() {
        bail!("generated output source is not a real directory");
    }
    let mut manifest = Vec::new();
    let mut files = Vec::new();
    let mut directories = Vec::new();
    if !describe_tree(
        source,
        Path::new(""),
        &mut manifest,
        &mut files,
        &mut directories,
    )? {
        bail!("generated output source has unsupported entries");
    }
    Ok((manifest, files, directories))
}

pub(crate) fn validate_source(source: &Path, snapshot: &Snapshot, root: &Path) -> Result<()> {
    validate_snapshot(snapshot)?;
    if snapshot.root != root {
        bail!("generated output owner root relocation is unavailable");
    }
    let (manifest, files, _) = describe(source)?;
    if CacheDigest::blake3(&manifest) != snapshot.digest {
        bail!("generated output source differs from its owner digest");
    }
    let expected = snapshot
        .files
        .iter()
        .map(|file| &file.path)
        .collect::<BTreeSet<_>>();
    if files.iter().map(|(file, _)| file).collect::<BTreeSet<_>>() != expected {
        bail!("generated output timestamp metadata differs from source files");
    }
    Ok(())
}

pub(crate) fn source_matches(source: &Path, snapshot: &Snapshot, root: &Path) -> Result<bool> {
    validate_snapshot(snapshot)?;
    if snapshot.root != root {
        bail!("generated output owner root relocation is unavailable");
    }
    let (manifest, _, _) = describe(source)?;
    Ok(CacheDigest::blake3(&manifest) == snapshot.digest)
}

fn modified(file: &FileTime) -> Result<SystemTime> {
    UNIX_EPOCH
        .checked_add(Duration::new(file.modified_secs, file.modified_nanos))
        .ok_or_else(|| eyre::eyre!("generated output timestamp overflows"))
}

fn validate_existing(stable: &Path, snapshot: &Snapshot) -> Result<()> {
    validate_source(stable, snapshot, &snapshot.root)?;
    let (_, files, directories) = describe(stable)?;
    for (file, executable) in files {
        protected(&stable.join(file), executable)?;
    }
    protected(stable, true)?;
    for directory in directories {
        protected(&stable.join(directory), true)?;
    }
    for file in &snapshot.files {
        if std::fs::metadata(stable.join(&file.path))?.modified()? != modified(file)? {
            bail!("generated output owner timestamp differs");
        }
    }
    Ok(())
}

fn protected(path: &Path, executable: bool) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let expected = if executable { 0o555 } else { 0o444 };
        if metadata.permissions().mode() & 0o7777 != expected {
            bail!("generated output owner view is not immutable");
        }
    }
    #[cfg(not(unix))]
    if metadata.is_file() && !metadata.permissions().readonly() {
        let _ = executable;
        bail!("generated output owner view is not immutable");
    }
    Ok(())
}

pub(crate) fn hydrate(source: &Path, snapshot: &Snapshot, root: &Path) -> Result<PathBuf> {
    roots::validate_root(root)?;
    validate_source(source, snapshot, root)?;
    if std::fs::symlink_metadata(root).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        bail!("generated output owner root is a symlink");
    }
    let held = LEASES
        .lock()
        .unwrap()
        .contains_key(&leases_dir(root, &snapshot.digest.hash));
    lease(root, &snapshot.digest.hash)?;
    let stable = root.join(&snapshot.digest.hash);
    let result = (|| -> Result<()> {
        let mut publishing =
            fslock::LockFile::open(&publish_lock_path(root, &snapshot.digest.hash))?;
        publishing.lock()?;
        if std::fs::symlink_metadata(&stable).is_ok() {
            // Never replace a possibly leased immutable tree, including corrupt ones.
            return validate_existing(&stable, snapshot);
        }
        let (manifest, files, directories) = describe(source)?;
        if CacheDigest::blake3(&manifest) != snapshot.digest {
            bail!("generated output source changed before hydration");
        }
        let times = snapshot
            .files
            .iter()
            .map(|file| Ok((file.path.clone(), modified(file)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        materialize(
            source,
            root,
            &stable,
            &manifest,
            &files,
            &directories,
            Some(&times),
        )?;
        validate_existing(&stable, snapshot)?;
        stamp_use(root, &snapshot.digest.hash);
        Ok(())
    })();
    if let Err(error) = result {
        if !held {
            release(root, &snapshot.digest.hash);
        }
        return Err(error);
    }
    Ok(stable)
}

#[cfg(test)]
#[path = "out_dir_state_tests.rs"]
mod tests;
