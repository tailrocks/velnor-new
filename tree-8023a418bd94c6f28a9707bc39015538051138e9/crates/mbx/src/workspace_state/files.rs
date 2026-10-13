use super::*;

pub(super) fn unpack_inline(
    cas: &LocalCas,
    digest: &CacheDigest,
    destination: &Path,
) -> Result<()> {
    let path = cas
        .find(digest)?
        .ok_or_else(|| eyre::eyre!("workspace-state inline archive is missing"))?;
    let mut archive = tar::Archive::new(File::open(path)?);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        validate_relative_path(&path)?;
        let kind = entry.header().entry_type();
        if !kind.is_file() && !kind.is_dir() && !kind.is_gnu_sparse() {
            bail!("workspace-state archive contains a non-file entry");
        }
        if !entry.unpack_in(destination)? {
            bail!("workspace-state archive contains an unsafe path");
        }
    }
    Ok(())
}

pub(super) fn materialize_references(
    cas: &LocalCas,
    executable: &Path,
    destination: &Path,
    references: &[FileReference],
) -> Result<u64> {
    let sources = references
        .iter()
        .map(|reference| {
            let source = match &reference.source {
                FileSource::Cas(digest) => cas
                    .find(digest)?
                    .ok_or_else(|| eyre::eyre!("workspace-state referenced object is missing"))?,
                FileSource::Mbx => executable.to_path_buf(),
            };
            let size = std::fs::metadata(&source)?.len();
            Ok((reference, source, size))
        })
        .collect::<Result<Vec<_>>>()?;
    let workers = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(sources.len().max(1));
    let next = AtomicUsize::new(0);
    let error = Mutex::new(None);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some((reference, source, _)) = sources.get(index) else {
                        break;
                    };
                    if error.lock().unwrap().is_some() {
                        break;
                    }
                    let result = restore_reference(destination, reference, source);
                    if let Err(found) = result {
                        *error.lock().unwrap() = Some(found);
                        break;
                    }
                }
            });
        }
    });
    if let Some(error) = error.into_inner().unwrap() {
        return Err(error);
    }
    Ok(sources.iter().map(|(_, _, size)| size).sum())
}

pub(super) fn restore_reference(
    root: &Path,
    reference: &FileReference,
    source: &Path,
) -> Result<()> {
    let destination = root.join(&reference.path);
    let copied = reflink_copy::reflink_or_copy(source, &destination)?;
    let _ = copied;
    set_file_metadata(
        &destination,
        reference.mode,
        reference.modified_secs,
        reference.modified_nanos,
    )
}

pub(super) fn restore_symlinks(root: &Path, links: &[Symlink]) -> Result<()> {
    for link in links {
        let destination = root.join(&link.path);
        create_symlink(&link.target, &destination, link.directory)?;
    }
    Ok(())
}
