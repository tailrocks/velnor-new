// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

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
        buffer: vec![0; STREAM_CHUNK_BYTES].into_boxed_slice(),
    };
    let stream = stream::try_unfold(
        (state, stream_fully_read),
        |(mut state, fully_read)| async move {
            let count = state.file.read(state.buffer.as_mut()).await?;
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
            state.hasher.update(&state.buffer[..count]);
            Ok(Some((
                Bytes::copy_from_slice(&state.buffer[..count]),
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
    let mut buffer = vec![0; STREAM_CHUNK_BYTES].into_boxed_slice();
    loop {
        let count = file
            .read(buffer.as_mut())
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
    buffer: Box<[u8]>,
}
