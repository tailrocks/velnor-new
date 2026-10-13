//! Resolve configured owner roots once, before nested compiler working directories.
use super::*;

pub(super) fn validate_root(root: &Path) -> Result<()> {
    if resolve_root(root)?.as_os_str() != root.as_os_str() {
        return Err(OwnerUnavailable("configured generated output root geometry changed").into());
    }
    Ok(())
}

pub(crate) fn resolve_root(path: &Path) -> Result<PathBuf> {
    let mut existing = std::path::absolute(path)?;
    let mut suffix = Vec::new();
    while !existing.try_exists()? {
        if std::fs::symlink_metadata(&existing)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            bail!("generated output root crosses a dangling symlink");
        }
        let component = existing
            .components()
            .next_back()
            .ok_or_else(|| eyre::eyre!("generated output root has no existing ancestor"))?;
        let std::path::Component::Normal(name) = component else {
            bail!("generated output root has an unresolved non-directory suffix");
        };
        suffix.push(name.to_owned());
        if !existing.pop() {
            bail!("generated output root has no existing ancestor");
        }
    }
    if !std::fs::metadata(&existing)?.is_dir() {
        bail!("generated output root ancestor is not a directory");
    }
    let mut root = existing.canonicalize()?;
    for component in suffix.into_iter().rev() {
        root.push(component);
    }
    Ok(root)
}
