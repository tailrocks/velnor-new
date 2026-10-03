use super::{digest, publication};
use eyre::{Result, bail};
use std::path::{Path, PathBuf};

const NAMESPACE: &str = "owned";
pub(super) const OWNER_FILE: &str = "mbx-owner";

pub(super) fn root(configured: &Path, source: &Path) -> Result<(PathBuf, String)> {
    let absolute = std::path::absolute(configured)?;
    let parent = absolute
        .parent()
        .ok_or_else(|| eyre::eyre!("shim root lacks parent"))?;
    std::fs::create_dir_all(parent)?;
    let configured = parent.canonicalize()?.join(
        absolute
            .file_name()
            .ok_or_else(|| eyre::eyre!("shim root lacks name"))?,
    );
    publication::directory(&configured, false)?;
    let namespace = configured.join(NAMESPACE);
    publication::directory(&namespace, true)?;
    let metadata = publication::source_metadata(source)?;
    let mut file = publication::open_plain(source)?;
    if !super::same_file(&metadata, &file.metadata()?) {
        bail!("owned source changed before identity capture");
    }
    let expected = digest(&mut file)?;
    if !super::same_file(&metadata, &file.metadata()?)
        || !super::same_file(&metadata, &std::fs::symlink_metadata(source)?)
    {
        bail!("owned source changed during identity capture");
    }
    let root = namespace.join(&expected);
    publication::directory(&root, true)?;
    Ok((root, expected))
}

/// A lazy role in a durable namespace uses that namespace's first owner bytes.
pub(super) fn fixed_source(destination: &Path) -> Result<Option<(PathBuf, String)>> {
    let absolute = std::path::absolute(destination)?;
    for root in absolute.ancestors().skip(1) {
        let Some(hash) = root.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if hash.len() != 64
            || !hash
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            continue;
        }
        if root
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            != Some(NAMESPACE)
        {
            continue;
        }
        if root.canonicalize()? != root {
            bail!("durable snapshot namespace contains an alias");
        }
        publication::directory(root, true)?;
        let owner = root.join(OWNER_FILE);
        publication::verify(&owner, hash, 0o500)?;
        return Ok(Some((owner, hash.into())));
    }
    Ok(None)
}
