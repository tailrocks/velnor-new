fn reject_symlink_directory(path: &Path) -> io::Result<()> {
    validate_private_directory_path(path)
}

fn reject_real_private_directory(path: &Path, created: bool) -> io::Result<()> {
    if created {
        let metadata = fs::symlink_metadata(path)?;
        require_owned_directory(&metadata, effective_uid())?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    reject_symlink_directory(path)
}

pub(crate) fn validate_private_directory_path(path: &Path) -> io::Result<()> {
    validate_private_directory_path_for_uid(path, effective_uid())
}

fn validate_path_for_creation(path: &Path) -> io::Result<()> {
    let absolute = absolute_lexical_path(path)?;
    let components = normal_components(&absolute)?;
    let mut current = PathBuf::new();
    for (index, component) in components.iter().enumerate() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                let is_leaf = index + 1 == components.len();
                require_directory_component(&metadata, effective_uid(), is_leaf)?;
            }
            Err(error)
                if error.kind() == io::ErrorKind::NotFound && index + 1 == components.len() =>
            {
                return Ok(());
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

pub(crate) fn validate_private_directory_path_for_uid(
    path: &Path,
    expected_uid: u32,
) -> io::Result<()> {
    let absolute = absolute_lexical_path(path)?;
    let components = normal_components(&absolute)?;
    let mut current = PathBuf::new();
    for (index, component) in components.iter().enumerate() {
        current.push(component);
        let metadata = fs::symlink_metadata(&current)?;
        require_directory_component(&metadata, expected_uid, index + 1 == components.len())?;
    }
    Ok(())
}

fn absolute_lexical_path(path: &Path) -> io::Result<PathBuf> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    if absolute
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
    {
        return Err(io::Error::other("cache path is not lexically canonical"));
    }
    Ok(absolute)
}

fn normal_components(path: &Path) -> io::Result<Vec<PathBuf>> {
    if !path.is_absolute() {
        return Err(io::Error::other("cache path must be absolute"));
    }
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::RootDir => components.push(PathBuf::from("/")),
            Component::Normal(name) => components.push(PathBuf::from(name)),
            _ => return Err(io::Error::other("cache path has invalid component")),
        }
    }
    Ok(components)
}

fn require_directory_component(
    metadata: &fs::Metadata,
    expected_uid: u32,
    leaf: bool,
) -> io::Result<()> {
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::other(
            "private path contains a non-directory component",
        ));
    }
    let mode = metadata.permissions().mode();
    let uid = metadata.uid();
    if leaf {
        if uid != expected_uid || mode & 0o077 != 0 {
            return Err(io::Error::other(
                "private directory has unexpected owner or permissions",
            ));
        }
    } else if uid != expected_uid && uid != 0 {
        return Err(io::Error::other(
            "private path ancestor has unexpected owner",
        ));
    } else if mode & 0o022 != 0 && !(uid == 0 && mode & 0o1000 != 0) {
        return Err(io::Error::other(
            "private path ancestor is writable by other principals",
        ));
    }
    Ok(())
}

fn require_owned_directory(metadata: &fs::Metadata, expected_uid: u32) -> io::Result<()> {
    if metadata.file_type().is_symlink() || !metadata.is_dir() || metadata.uid() != expected_uid {
        return Err(io::Error::other(
            "private directory has unexpected owner or type",
        ));
    }
    Ok(())
}

fn effective_uid() -> u32 {
    rustix::process::geteuid().as_raw()
}

fn reject_regular_private(file: &File) -> io::Result<()> {
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != effective_uid()
        || metadata.permissions().mode() & 0o777 != 0o600
    {
        return Err(io::Error::other(
            "cache lock must be a mode-0600 regular file owned by the service",
        ));
    }
    Ok(())
}
