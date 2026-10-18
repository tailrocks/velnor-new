//! Bounded, content-checked file streams for Docker image import.

use std::fs::OpenOptions;
use std::io;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bytes::Bytes;
use futures_util::stream;
use futures_util::{StreamExt, stream::BoxStream};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};

use super::{ProviderFailure, hex, valid_hex};

const STREAM_CHUNK_BYTES: usize = 64 * 1024;

pub(super) async fn open_verified_archive(
    path: &Path,
    expected_sha256: &str,
    max_bytes: u64,
) -> Result<VerifiedArchiveStream, ProviderFailure> {
    if !valid_hex(expected_sha256, 64) {
        return Err(ProviderFailure::Archive);
    }
    let path_metadata = std::fs::symlink_metadata(path).map_err(|_| ProviderFailure::Archive)?;
    if !path_metadata.file_type().is_file()
        || path_metadata.len() == 0
        || path_metadata.len() > max_bytes
    {
        return Err(ProviderFailure::Archive);
    }

    let file = open_without_following_links(path)?;
    let metadata = file.metadata().map_err(|_| ProviderFailure::Archive)?;
    if !metadata.is_file() || metadata.len() != path_metadata.len() || metadata.len() > max_bytes {
        return Err(ProviderFailure::Archive);
    }
    let mut file = tokio::fs::File::from_std(file);
    let (observed_sha256, observed_size) = hash_and_rewind(&mut file, max_bytes).await?;
    if observed_sha256 != expected_sha256 || observed_size != metadata.len() {
        return Err(ProviderFailure::Archive);
    }

    let fully_read = Arc::new(AtomicBool::new(false));
    let stream_fully_read = Arc::clone(&fully_read);
    let state = UploadState {
        file,
        expected_sha256: expected_sha256.to_owned(),
        expected_size: metadata.len(),
        max_bytes,
        sent: 0,
        hasher: Sha256::new(),
    };
    let stream = stream::try_unfold(
        (state, stream_fully_read),
        |(mut state, fully_read)| async move {
            let mut buffer = [0; STREAM_CHUNK_BYTES];
            let count = state.file.read(&mut buffer).await?;
            if count == 0 {
                if state.sent != state.expected_size
                    || hex(&state.hasher.finalize()) != state.expected_sha256
                {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "worker archive changed during import",
                    ));
                }
                fully_read.store(true, Ordering::Release);
                return Ok(None);
            }
            state.sent = state
                .sent
                .checked_add(u64::try_from(count).map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidData, "worker archive size overflow")
                })?)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "worker archive overflow")
                })?;
            if state.sent > state.expected_size || state.sent > state.max_bytes {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "worker archive exceeds its verified size",
                ));
            }
            state.hasher.update(&buffer[..count]);
            Ok(Some((
                Bytes::copy_from_slice(&buffer[..count]),
                (state, fully_read),
            )))
        },
    )
    .boxed();
    Ok(VerifiedArchiveStream { stream, fully_read })
}

pub(super) struct VerifiedArchiveStream {
    pub(super) stream: BoxStream<'static, Result<Bytes, io::Error>>,
    fully_read: Arc<AtomicBool>,
}

impl VerifiedArchiveStream {
    pub(super) fn completion_token(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.fully_read)
    }
}

fn open_without_following_links(path: &Path) -> Result<std::fs::File, ProviderFailure> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options.open(path).map_err(|_| ProviderFailure::Archive)
}

async fn hash_and_rewind(
    file: &mut tokio::fs::File,
    max_bytes: u64,
) -> Result<(String, u64), ProviderFailure> {
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0; STREAM_CHUNK_BYTES];
    loop {
        let count = file
            .read(&mut buffer)
            .await
            .map_err(|_| ProviderFailure::Archive)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(u64::try_from(count).map_err(|_| ProviderFailure::Archive)?)
            .ok_or(ProviderFailure::Archive)?;
        if total > max_bytes {
            return Err(ProviderFailure::Archive);
        }
        hasher.update(&buffer[..count]);
    }
    file.seek(SeekFrom::Start(0))
        .await
        .map_err(|_| ProviderFailure::Archive)?;
    Ok((hex(&hasher.finalize()), total))
}

struct UploadState {
    file: tokio::fs::File,
    expected_sha256: String,
    expected_size: u64,
    max_bytes: u64,
    sent: u64,
    hasher: Sha256,
}

#[cfg(test)]
mod tests {
    use std::io::{Seek, SeekFrom, Write};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use bytes::Bytes;
    use futures_util::TryStreamExt;
    use sha2::Digest;

    use super::{ProviderFailure, STREAM_CHUNK_BYTES, hex, open_verified_archive};

    static NEXT_PATH: AtomicU64 = AtomicU64::new(1);

    #[tokio::test]
    async fn streams_verified_archive_in_bounded_chunks() -> Result<(), String> {
        let bytes = vec![0x5a; STREAM_CHUNK_BYTES * 2 + 17];
        let path = write_fixture(&bytes);
        let archive = open_verified_archive(
            &path,
            &hex(&sha2::Sha256::digest(&bytes)),
            bytes.len() as u64,
        )
        .await
        .map_err(|error| format!("open verified archive: {error:?}"))?;
        let fully_read = archive.completion_token();
        let chunks: Vec<_> = archive
            .stream
            .map_ok(|chunk| chunk.to_vec())
            .try_collect()
            .await
            .map_err(|error| error.to_string())?;

        assert!(chunks.iter().all(|chunk| chunk.len() <= STREAM_CHUNK_BYTES));
        assert_eq!(chunks.concat(), bytes);
        assert!(fully_read.load(Ordering::Acquire));
        std::fs::remove_file(path).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[tokio::test]
    async fn archive_mutation_during_stream_is_rejected_before_eof() -> Result<(), String> {
        let bytes = vec![0x44; STREAM_CHUNK_BYTES * 2];
        let path = write_fixture(&bytes);
        let archive = open_verified_archive(
            &path,
            &hex(&sha2::Sha256::digest(&bytes)),
            bytes.len() as u64,
        )
        .await
        .map_err(|error| format!("open verified archive: {error:?}"))?;
        let fully_read = archive.completion_token();
        let mut stream = archive.stream;
        assert!(
            stream
                .try_next()
                .await
                .map_err(|error| error.to_string())?
                .is_some()
        );
        assert!(!fully_read.load(Ordering::Acquire));

        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .map_err(|error| error.to_string())?;
        file.seek(SeekFrom::Start((STREAM_CHUNK_BYTES + 1) as u64))
            .map_err(|error| error.to_string())?;
        file.write_all(&[0x55]).map_err(|error| error.to_string())?;
        file.flush().map_err(|error| error.to_string())?;

        let result: Result<Vec<Bytes>, _> = stream.try_collect().await;
        assert!(result.is_err());
        assert!(!fully_read.load(Ordering::Acquire));
        std::fs::remove_file(path).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[tokio::test]
    async fn rejects_wrong_digest_and_oversized_file_before_streaming() -> Result<(), String> {
        let bytes = vec![0x7b; 32];
        let path = write_fixture(&bytes);
        assert!(matches!(
            open_verified_archive(&path, &"0".repeat(64), 1024).await,
            Err(ProviderFailure::Archive)
        ));
        assert!(matches!(
            open_verified_archive(&path, &hex(&sha2::Sha256::digest(&bytes)), 31).await,
            Err(ProviderFailure::Archive)
        ));
        std::fs::remove_file(path).map_err(|error| error.to_string())?;
        Ok(())
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_a_symlink_archive() -> Result<(), String> {
        use std::os::unix::fs::symlink;

        let bytes = vec![0x33; 32];
        let target = write_fixture(&bytes);
        let link = next_path("link");
        symlink(&target, &link).map_err(|error| error.to_string())?;
        assert!(matches!(
            open_verified_archive(&link, &hex(&sha2::Sha256::digest(&bytes)), 1024).await,
            Err(ProviderFailure::Archive)
        ));
        std::fs::remove_file(link).map_err(|error| error.to_string())?;
        std::fs::remove_file(target).map_err(|error| error.to_string())?;
        Ok(())
    }

    fn write_fixture(bytes: &[u8]) -> PathBuf {
        let path = next_path("archive");
        std::fs::write(&path, bytes).expect("stream fixture is written");
        path
    }

    fn next_path(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "velnor-worker-upload-{label}-{}-{}",
            std::process::id(),
            NEXT_PATH.fetch_add(1, Ordering::Relaxed)
        ))
    }
}
