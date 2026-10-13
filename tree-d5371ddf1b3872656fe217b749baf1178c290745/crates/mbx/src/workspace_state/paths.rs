use super::*;

pub(super) fn normalized_relative_path(path: &Path) -> Result<String> {
    validate_relative_path(path)?;
    normalized_path(path)
}

pub(super) fn normalized_link_target(path: &Path) -> Result<String> {
    if path.is_absolute() {
        bail!("workspace state contains an absolute link target");
    }
    normalized_path(path)
}

pub(super) fn normalized_path(path: &Path) -> Result<String> {
    let mut normalized = String::new();
    for component in path.components() {
        let component = match component {
            Component::Normal(component) => component,
            Component::CurDir => {
                if !normalized.is_empty() {
                    normalized.push('/');
                }
                normalized.push('.');
                continue;
            }
            Component::ParentDir => {
                if !normalized.is_empty() {
                    normalized.push('/');
                }
                normalized.push_str("..");
                continue;
            }
            _ => bail!("workspace state contains an unsafe path {}", path.display()),
        };
        let component = component
            .to_str()
            .ok_or_else(|| eyre::eyre!("workspace state path is not valid UTF-8"))?;
        if !normalized.is_empty() {
            normalized.push('/');
        }
        normalized.push_str(component);
    }
    if normalized.is_empty() {
        bail!("workspace state contains an empty path");
    }
    Ok(normalized)
}

pub(super) fn validate_absolute_root(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        || path.components().collect::<PathBuf>().as_os_str() != path.as_os_str()
    {
        bail!(
            "Cargo root must be an absolute normalized path: {}",
            path.display()
        );
    }
    Ok(())
}
