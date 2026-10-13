//! Materialize an action-proven immutable view from native referenced objects.
use super::*;
use mbx_cache_core::LocalCas;

pub(crate) fn validate_cas(cas: &LocalCas, snapshot: &Snapshot) -> Result<()> {
    validate_snapshot(snapshot)?;
    for file in &snapshot.files {
        cas.find(&file.digest)?
            .ok_or_else(|| eyre::eyre!("generated output native object is missing"))?;
    }
    Ok(())
}

pub(crate) fn hydrate_cas(cas: &LocalCas, snapshot: &Snapshot, root: &Path) -> Result<PathBuf> {
    validate_snapshot(snapshot)?;
    if snapshot.root != root {
        bail!("generated output owner root relocation is unavailable");
    }
    roots::validate_root(root)?;
    let sources = snapshot
        .files
        .iter()
        .map(|file| {
            let source = cas
                .find(&file.digest)?
                .ok_or_else(|| eyre::eyre!("generated output native object is missing"))?;
            Ok((file, source))
        })
        .collect::<Result<Vec<_>>>()?;
    let staging = tempfile::tempdir()?;
    for directory in &snapshot.directories {
        std::fs::create_dir_all(staging.path().join(directory))?;
    }
    for (file, source) in sources {
        let destination = staging.path().join(&file.path);
        let _copied = reflink_copy::reflink_or_copy(source, &destination)?;
        make_read_only(&destination, file.executable)?;
    }
    hydrate(staging.path(), snapshot, root)
}

pub(super) fn store_view(cas: &LocalCas, snapshot: &Snapshot) -> Result<()> {
    let stable = snapshot.root.join(&snapshot.digest.hash);
    validate_existing(&stable, snapshot)?;
    for file in &snapshot.files {
        cas.store_file(&file.digest, &stable.join(&file.path))?;
    }
    Ok(())
}
