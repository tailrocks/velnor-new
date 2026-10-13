fn read_regular_file(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("cache file has no parent directory"))?;
    validate_private_directory_path(parent)?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.uid() != effective_uid()
        || metadata.permissions().mode() & 0o777 != 0o600
        || metadata.len() > limit
    {
        return Err(io::Error::other("cache path is not a bounded regular file"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW);
    let file = options.open(path)?;
    let opened_metadata = file.metadata()?;
    if !opened_metadata.is_file()
        || opened_metadata.uid() != effective_uid()
        || opened_metadata.dev() != metadata.dev()
        || opened_metadata.ino() != metadata.ino()
        || opened_metadata.permissions().mode() & 0o777 != 0o600
        || opened_metadata.len() > limit
    {
        return Err(io::Error::other("cache file is not private and bounded"));
    }
    let capacity = usize::try_from(metadata.len())
        .map_err(|_| io::Error::other("cache file size does not fit this target"))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::other("cache file exceeds bound"));
    }
    Ok(bytes)
}

fn read_bounded(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    read_regular_file(path, limit)
}

fn write_file(path: &Path, bytes: &[u8], mode: u32) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        validate_private_directory_path(parent)?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(mode).custom_flags(libc::O_NOFOLLOW);
    let mut file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.uid() != effective_uid() {
        return Err(io::Error::other(
            "new cache file has unexpected owner or type",
        ));
    }
    file.set_permissions(fs::Permissions::from_mode(mode))?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn sync_tree(path: &Path) -> io::Result<()> {
    validate_private_directory_path(path)?;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        let metadata = fs::symlink_metadata(&child)?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::other("symlink in generation"));
        }
        if metadata.is_dir() {
            sync_tree(&child)?;
        } else if metadata.is_file() {
            require_private_file(&metadata)?;
            File::open(&child)?.sync_all()?;
        } else {
            return Err(io::Error::other("non-regular generation entry"));
        }
    }
    sync_directory(path)
}

fn secure_tree(path: &Path) -> io::Result<()> {
    require_owned_directory(&fs::symlink_metadata(path)?, effective_uid())?;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let child = entry.path();
        let metadata = fs::symlink_metadata(&child)?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::other("symlink in TUF datastore"));
        }
        if metadata.is_dir() {
            secure_tree(&child)?;
            fs::set_permissions(&child, fs::Permissions::from_mode(0o700))?;
        } else if metadata.is_file() {
            require_owned_file(&metadata, false)?;
            fs::set_permissions(&child, fs::Permissions::from_mode(0o600))?;
        } else {
            return Err(io::Error::other("non-regular TUF datastore entry"));
        }
    }
    Ok(())
}

fn sync_directory(path: &Path) -> io::Result<()> {
    validate_private_directory_path(path)?;
    File::open(path)?.sync_all()
}

fn hash_tree_files(path: &Path) -> io::Result<BTreeMap<String, FileDigest>> {
    validate_private_directory_path(path)?;
    let mut files = BTreeMap::new();
    let mut total = 0_u64;
    hash_directory(path, path, &mut total, &mut files)?;
    Ok(files)
}

fn hash_directory(
    root: &Path,
    directory: &Path,
    total: &mut u64,
    files: &mut BTreeMap<String, FileDigest>,
) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(io::Error::other("symlink in generation"));
        }
        if metadata.is_dir() {
            validate_private_directory_path(&path)?;
            hash_directory(root, &path, total, files)?;
            continue;
        }
        if !metadata.is_file() || metadata.len() > MAX_METADATA_FILE_BYTES {
            return Err(io::Error::other("invalid generation file"));
        }
        require_private_file(&metadata)?;
        *total = total.saturating_add(metadata.len());
        if *total > MAX_CACHE_BYTES {
            return Err(io::Error::other("TUF generation exceeds total byte bound"));
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| io::Error::other("generation path escaped root"))?;
        validate_relative_path(relative)?;
        if relative == Path::new(MANIFEST_FILE) {
            continue;
        }
        let bytes = read_regular_file(&path, MAX_METADATA_FILE_BYTES)?;
        files.insert(
            relative.to_string_lossy().replace('\\', "/"),
            FileDigest {
                size: bytes.len() as u64,
                sha256: sha256_hex(&bytes),
            },
        );
    }
    Ok(())
}

fn require_private_file(metadata: &fs::Metadata) -> io::Result<()> {
    require_owned_file(metadata, false)?;
    if metadata.permissions().mode() & 0o777 != 0o600 {
        return Err(io::Error::other("cache file is not mode 0600"));
    }
    Ok(())
}

fn require_owned_file(metadata: &fs::Metadata, executable: bool) -> io::Result<()> {
    let mode = metadata.permissions().mode();
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.uid() != effective_uid()
        || mode & 0o022 != 0
        || (executable && mode & 0o100 == 0)
    {
        return Err(io::Error::other("file has unexpected owner, type, or mode"));
    }
    Ok(())
}

fn validate_relative_path(path: &Path) -> io::Result<()> {
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(io::Error::other("invalid generation relative path"));
        }
    }
    Ok(())
}

fn manifest_tuf_files(manifest: &GenerationManifest) -> io::Result<BTreeMap<String, String>> {
    let mut tuf_files = BTreeMap::new();
    for (path, file) in &manifest.files {
        if let Some(name) = path.strip_prefix("tuf/") {
            if name.contains('/') || name.is_empty() {
                return Err(io::Error::other("unexpected nested TUF metadata file"));
            }
            tuf_files.insert(name.to_owned(), file.sha256.clone());
        }
    }
    if !tuf_files.contains_key("root.json") {
        return Err(io::Error::other("TUF root metadata missing"));
    }
    Ok(tuf_files)
}

fn hash_tuf_files(path: &Path) -> io::Result<BTreeMap<String, String>> {
    Ok(hash_tree_files(path)?
        .into_iter()
        .map(|(name, digest)| (name, digest.sha256))
        .collect())
}

fn unique_generation_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("gen-{}-{nanos}", std::process::id())
}

fn valid_generation_id(value: &str) -> bool {
    value.starts_with("gen-")
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity(64);
    for byte in digest {
        output.push(HEX[usize::from(byte >> 4)] as char);
        output.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    output
}
