//! Data-only verification of the owner's generated-output identity.
use super::*;

pub(crate) fn inventory_matches(
    snapshot: &Snapshot,
    directories: &BTreeSet<PathBuf>,
    files: &BTreeMap<PathBuf, (CacheDigest, bool)>,
) -> Result<bool> {
    validate_snapshot(snapshot)?;
    Ok(inventory_digest(directories, files)? == snapshot.digest)
}

pub(crate) fn validate_inventory(
    snapshot: &Snapshot,
    directories: &BTreeSet<PathBuf>,
    files: &BTreeMap<PathBuf, (CacheDigest, bool)>,
) -> Result<()> {
    validate_snapshot(snapshot)?;
    if directories
        != &snapshot
            .directories
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>()
        || files
            != &snapshot
                .files
                .iter()
                .map(|file| (file.path.clone(), (file.digest.clone(), file.executable)))
                .collect::<BTreeMap<_, _>>()
    {
        bail!("generated output inventory differs from owner file declarations");
    }
    verify_manifest(snapshot, directories, files)
}

pub(super) fn validate_declared(snapshot: &Snapshot) -> Result<()> {
    verify_manifest(
        snapshot,
        &snapshot.directories.iter().cloned().collect(),
        &snapshot
            .files
            .iter()
            .map(|file| (file.path.clone(), (file.digest.clone(), file.executable)))
            .collect(),
    )
}

fn verify_manifest(
    snapshot: &Snapshot,
    directories: &BTreeSet<PathBuf>,
    files: &BTreeMap<PathBuf, (CacheDigest, bool)>,
) -> Result<()> {
    if inventory_digest(directories, files)? != snapshot.digest {
        bail!("generated output inventory differs from its owner digest");
    }
    Ok(())
}

fn inventory_digest(
    directories: &BTreeSet<PathBuf>,
    files: &BTreeMap<PathBuf, (CacheDigest, bool)>,
) -> Result<CacheDigest> {
    let mut children: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for path in directories.iter().chain(files.keys()) {
        relative(path)?;
        let parent = path
            .parent()
            .ok_or_else(|| eyre::eyre!("generated output entry has no parent"))?;
        if !parent.as_os_str().is_empty() && !directories.contains(parent) {
            bail!("generated output inventory has a missing directory ancestor");
        }
        children
            .entry(parent.to_path_buf())
            .or_default()
            .push(path.clone());
    }
    if directories.iter().any(|path| files.contains_key(path)) {
        bail!("generated output inventory has overlapping entries");
    }
    for (digest, _) in files.values() {
        digest.validate()?;
        if digest.algorithm != "blake3" {
            bail!("generated output file identity is not blake3");
        }
    }
    for entries in children.values_mut() {
        entries.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    }
    let mut pending = children.get(Path::new("")).cloned().unwrap_or_default();
    pending.reverse();
    let mut manifest = Vec::new();
    while let Some(path) = pending.pop() {
        let spelled = path
            .to_str()
            .ok_or_else(|| eyre::eyre!("generated output path is not UTF-8"))?
            .replace('\\', "/");
        if directories.contains(&path) {
            manifest.extend_from_slice(b"d ");
        } else {
            let (digest, executable) = files
                .get(&path)
                .ok_or_else(|| eyre::eyre!("generated output inventory is incomplete"))?;
            manifest.extend_from_slice(if *executable { b"x " } else { b"f " });
            manifest.extend_from_slice(digest.hash.as_bytes());
            manifest.push(b' ');
        }
        manifest.extend_from_slice(format!("{}:", spelled.len()).as_bytes());
        manifest.extend_from_slice(spelled.as_bytes());
        manifest.push(b'\n');
        if let Some(entries) = children.get(&path) {
            pending.extend(entries.iter().rev().cloned());
        }
    }
    Ok(CacheDigest::blake3(&manifest))
}
