//! Immutable, secret-free worker image metadata mounted into the runner.

mod document;
mod storage;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::config::{ConnectionId, RepositoryConnection};
use crate::error::HostError;
use crate::launch::VerifiedWorkerImages;
use crate::worker::BindMount;

use self::document::GuestImageDocument;
use self::storage::{publish, verify_path};

pub(super) const MAX_DOCUMENT_BYTES: u64 = 16 * 1024;

/// One immutable guest metadata file tied to an exact connection and image bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuestImageFile {
    state_root: PathBuf,
    path: PathBuf,
    connection_id: ConnectionId,
    binding_digest: [u8; 32],
    document_bytes: Vec<u8>,
}

impl GuestImageFile {
    /// Return the guest bind only while binding, image claims, and file bytes still match.
    pub(crate) fn runner_bind(
        &self,
        connection: &RepositoryConnection,
        images: &VerifiedWorkerImages,
    ) -> Result<BindMount, HostError> {
        let expected = GuestImageDocument::new(connection, images)?;
        let bytes = encode_document(&expected)?;
        let binding_digest = binding_digest(connection)?;
        let expected_path = content_path(&self.state_root, connection, &bytes)?;
        if self.connection_id != connection.connection_id
            || self.binding_digest != binding_digest
            || self.document_bytes != bytes
            || self.path != expected_path
        {
            return Err(HostError::ForbiddenMount);
        }
        verify_path(
            &self.state_root,
            connection.connection_id.as_str(),
            &self.path,
            &bytes,
        )?;
        let source = self.path.to_str().ok_or(HostError::Path)?.to_owned();
        Ok(BindMount::guest_image(source))
    }

    #[cfg(test)]
    fn path(&self) -> &Path {
        &self.path
    }
}

/// Write canonical guest metadata under a private, content-addressed state path.
pub(crate) fn write_guest_image(
    state_root: &Path,
    connection: &RepositoryConnection,
    images: &VerifiedWorkerImages,
) -> Result<GuestImageFile, HostError> {
    let document = GuestImageDocument::new(connection, images)?;
    let bytes = encode_document(&document)?;
    let path = content_path(state_root, connection, &bytes)?;
    publish(state_root, connection.connection_id.as_str(), &path, &bytes)?;
    Ok(GuestImageFile {
        state_root: state_root.to_path_buf(),
        path,
        connection_id: connection.connection_id.clone(),
        binding_digest: binding_digest(connection)?,
        document_bytes: bytes,
    })
}

pub(crate) fn validate_worker_images(images: &VerifiedWorkerImages) -> Result<(), HostError> {
    GuestImageDocument::validate_images(images)
}

fn encode_document(document: &GuestImageDocument) -> Result<Vec<u8>, HostError> {
    let bytes = serde_json::to_vec(document).map_err(|_| HostError::ForbiddenMount)?;
    if bytes.is_empty()
        || u64::try_from(bytes.len()).map_or(true, |length| length > MAX_DOCUMENT_BYTES)
    {
        return Err(HostError::ForbiddenMount);
    }
    Ok(bytes)
}

fn binding_digest(connection: &RepositoryConnection) -> Result<[u8; 32], HostError> {
    let bytes = connection.canonical_identity_bytes()?;
    Ok(Sha256::digest(bytes).into())
}

fn content_path(
    state_root: &Path,
    connection: &RepositoryConnection,
    bytes: &[u8],
) -> Result<PathBuf, HostError> {
    if !state_root.is_absolute() || connection.connection_id.as_str().is_empty() {
        return Err(HostError::Path);
    }
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    let name = format!("{}.json", hex_digest(&digest));
    Ok(state_root
        .join("guest-image")
        .join(connection.connection_id.as_str())
        .join(name))
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}
