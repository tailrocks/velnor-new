use std::io::{Read, Write};
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
        Ok(_) => return Err(StateWriteError::Failed),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(StateWriteError::Failed),
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
    let parent = path.parent().ok_or(StateWriteError::Failed)?;
    std::fs::create_dir_all(parent).map_err(|_| StateWriteError::Failed)?;
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
        Ok(_) => return Err(StateWriteError::Failed),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(StateWriteError::Failed),
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
    std::fs::create_dir_all(parent).map_err(|_| StateWriteError::Failed)?;
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
        Ok(_) => return Err(StateWriteError::Failed),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(StateWriteError::Failed),
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
        let _ = std::fs::remove_file(&temporary);
        return Err(StateWriteError::Failed);
    }
    drop(file);
    if std::fs::rename(&temporary, path).is_err() {
        let _ = std::fs::remove_file(&temporary);
        return Err(StateWriteError::Failed);
    }
    std::fs::File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| StateWriteError::Uncertain)
}

pub(super) fn report_marker_write_error(error: StateWriteError) {
    match error {
        StateWriteError::Failed => eprintln!("Drain request failed."),
        StateWriteError::Uncertain => eprintln!("Drain state is uncertain."),
    }
}
