use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StateWriteError {
    Failed,
    Uncertain,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MarkerToken(pub(super) String);

impl MarkerToken {
    pub(super) fn parse(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 32 || !bytes.iter().all(u8::is_ascii_hexdigit) {
            return None;
        }
        let token = std::str::from_utf8(bytes).ok()?;
        if token.bytes().any(|byte| byte.is_ascii_uppercase()) {
            return None;
        }
        Some(Self(token.to_owned()))
    }

    fn generate() -> Result<Self, StateWriteError> {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut random = [0_u8; 16];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut random_source| random_source.read_exact(&mut random))
            .map_err(|_| StateWriteError::Failed)?;
        let mut token = String::with_capacity(32);
        for byte in random {
            token.push(char::from(HEX[usize::from(byte >> 4)]));
            token.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
        Ok(Self(token))
    }
}

pub(super) fn marker_token(path: &Path) -> Result<Option<MarkerToken>, StateWriteError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Ok(_) | Err(_) => return Err(StateWriteError::Failed),
    }
    let mut bytes = Vec::with_capacity(33);
    std::fs::File::open(path)
        .and_then(|file| file.take(33).read_to_end(&mut bytes))
        .map_err(|_| StateWriteError::Failed)?;
    MarkerToken::parse(&bytes)
        .map(Some)
        .ok_or(StateWriteError::Failed)
}

pub(super) fn write_marker(path: &Path) -> Result<MarkerToken, StateWriteError> {
    let token = MarkerToken::generate()?;
    atomic_write(path, token.0.as_bytes())?;
    Ok(token)
}

pub(super) fn remove_marker_if_matches(
    path: &Path,
    expected: &MarkerToken,
) -> Result<bool, StateWriteError> {
    if marker_token(path)? != Some(expected.clone()) {
        return Ok(false);
    }
    remove_marker_path(path)
}

pub(super) fn remove_marker_path(path: &Path) -> Result<bool, StateWriteError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Ok(_) | Err(_) => return Err(StateWriteError::Failed),
    }
    std::fs::remove_file(path).map_err(|_| StateWriteError::Failed)?;
    let parent = path.parent().ok_or(StateWriteError::Failed)?;
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| StateWriteError::Uncertain)?;
    Ok(true)
}

pub(super) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), StateWriteError> {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let parent = path.parent().ok_or(StateWriteError::Failed)?;
    prepare_parent_directory(parent)?;
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Ok(_) | Err(_) => return Err(StateWriteError::Failed),
    }
    let name = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .ok_or(StateWriteError::Failed)?;
    let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".{name}.tmp-{}-{sequence}", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temporary)
        .map_err(|_| StateWriteError::Failed)?;
    if file.write_all(bytes).is_err() || file.sync_all().is_err() {
        drop(file);
        drop(std::fs::remove_file(&temporary));
        return Err(StateWriteError::Failed);
    }
    drop(file);
    if std::fs::rename(&temporary, path).is_err() {
        drop(std::fs::remove_file(&temporary));
        return Err(StateWriteError::Failed);
    }
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| StateWriteError::Uncertain)
}

fn prepare_parent_directory(parent: &Path) -> Result<(), StateWriteError> {
    if parent.file_name() != Some(std::ffi::OsStr::new("connection-drain")) {
        std::fs::create_dir_all(parent).map_err(|_| StateWriteError::Failed)?;
        return Ok(());
    }

    let state = parent.parent().ok_or(StateWriteError::Failed)?;
    std::fs::create_dir_all(state).map_err(|_| StateWriteError::Failed)?;
    let state_metadata = std::fs::symlink_metadata(state).map_err(|_| StateWriteError::Failed)?;
    if !state_metadata.is_dir()
        || state_metadata.file_type().is_symlink()
        || !owned_by_current_user(&state_metadata)
    {
        return Err(StateWriteError::Failed);
    }
    match std::fs::symlink_metadata(parent) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(parent).map_err(|_| StateWriteError::Failed)?;
        }
        Err(_) => return Err(StateWriteError::Failed),
    }
    let before = std::fs::symlink_metadata(parent).map_err(|_| StateWriteError::Failed)?;
    if !before.is_dir() || before.file_type().is_symlink() || !owned_by_current_user(&before) {
        return Err(StateWriteError::Failed);
    }
    let directory = open_directory_nofollow(parent)?;
    let opened = directory.metadata().map_err(|_| StateWriteError::Failed)?;
    if !same_directory(&before, &opened) || !opened.is_dir() || !owned_by_current_user(&opened) {
        return Err(StateWriteError::Failed);
    }
    if has_group_or_other_permissions(&opened) {
        directory
            .set_permissions(std::fs::Permissions::from_mode(0o700))
            .map_err(|_| StateWriteError::Failed)?;
    }
    let after = std::fs::symlink_metadata(parent).map_err(|_| StateWriteError::Failed)?;
    if !same_directory(&opened, &after)
        || !after.is_dir()
        || after.file_type().is_symlink()
        || !owned_by_current_user(&after)
        || has_group_or_other_permissions(&after)
    {
        return Err(StateWriteError::Failed);
    }
    Ok(())
}

fn open_directory_nofollow(path: &Path) -> Result<File, StateWriteError> {
    let flags = rustix::fs::OFlags::RDONLY
        | rustix::fs::OFlags::DIRECTORY
        | rustix::fs::OFlags::NOFOLLOW
        | rustix::fs::OFlags::CLOEXEC;
    let descriptor = rustix::fs::open(path, flags, rustix::fs::Mode::empty())
        .map_err(|_| StateWriteError::Failed)?;
    Ok(File::from(descriptor))
}

fn owned_by_current_user(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    metadata.uid() == rustix::process::geteuid().as_raw()
}

fn has_group_or_other_permissions(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    metadata.mode().trailing_zeros() < 6
}

fn same_directory(left: &std::fs::Metadata, right: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;

    left.dev() == right.dev() && left.ino() == right.ino()
}

pub(super) fn report_marker_write_error(error: StateWriteError) {
    match error {
        StateWriteError::Failed => eprintln!("Drain request failed."),
        StateWriteError::Uncertain => eprintln!("Drain state is uncertain."),
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{marker_token, write_marker};

    #[test]
    fn connection_marker_creation_enforces_private_directory_and_files() -> Result<(), String> {
        static NEXT_STATE: AtomicU64 = AtomicU64::new(0);
        let state = std::env::temp_dir().join(format!(
            "velnor-cli-marker-{}-{}",
            std::process::id(),
            NEXT_STATE.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&state).map_err(|error| error.to_string())?;
        let marker = state.join("connection-drain/essential-mac");
        let token = write_marker(&marker).map_err(|error| format!("marker: {error:?}"))?;
        let directory = std::fs::symlink_metadata(marker.parent().ok_or("marker has no parent")?)
            .map_err(|error| error.to_string())?;
        let file = std::fs::symlink_metadata(&marker).map_err(|error| error.to_string())?;
        if directory.mode().trailing_zeros() < 6
            || directory.mode() >> 6 & 0o7 != 0o7
            || directory.uid() != rustix::process::geteuid().as_raw()
            || file.mode().trailing_zeros() < 6
            || marker_token(&marker).map_err(|error| format!("read marker: {error:?}"))?
                != Some(token)
        {
            return Err("connection marker did not use private owned storage".to_owned());
        }
        std::fs::remove_dir_all(state).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn existing_public_directory_is_hardened_without_following_a_symlink() -> Result<(), String> {
        let state = test_state("existing");
        std::fs::create_dir(&state).map_err(|error| error.to_string())?;
        let directory = state.join("connection-drain");
        std::fs::create_dir(&directory).map_err(|error| error.to_string())?;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755))
            .map_err(|error| error.to_string())?;
        let marker = directory.join("essential-mac");
        write_marker(&marker).map_err(|error| format!("existing directory: {error:?}"))?;
        let hardened = std::fs::symlink_metadata(&directory).map_err(|error| error.to_string())?;
        if hardened.mode() & 0o777 != 0o700 {
            return Err("owned existing directory stayed broadly accessible".to_owned());
        }
        std::fs::remove_dir_all(&state).map_err(|error| error.to_string())?;

        let state = test_state("symlink");
        let target = state.with_extension("target");
        std::fs::create_dir(&state).map_err(|error| error.to_string())?;
        std::fs::create_dir(&target).map_err(|error| error.to_string())?;
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755))
            .map_err(|error| error.to_string())?;
        std::os::unix::fs::symlink(&target, state.join("connection-drain"))
            .map_err(|error| error.to_string())?;
        if write_marker(&state.join("connection-drain/essential-mac")).is_ok() {
            return Err("marker writer followed a directory symlink".to_owned());
        }
        let unchanged = std::fs::symlink_metadata(&target).map_err(|error| error.to_string())?;
        if unchanged.mode() & 0o777 != 0o755 {
            return Err("symlink target permissions changed".to_owned());
        }
        std::fs::remove_dir_all(state).map_err(|error| error.to_string())?;
        std::fs::remove_dir_all(target).map_err(|error| error.to_string())?;
        Ok(())
    }

    fn test_state(label: &str) -> std::path::PathBuf {
        static NEXT_STATE: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "velnor-cli-marker-{label}-{}-{}",
            std::process::id(),
            NEXT_STATE.fetch_add(1, Ordering::Relaxed)
        ))
    }
}
