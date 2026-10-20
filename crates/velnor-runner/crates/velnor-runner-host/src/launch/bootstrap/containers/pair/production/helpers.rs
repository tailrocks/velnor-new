use sha2::{Digest, Sha256};

use super::super::super::BootstrapContainerRole;
use crate::error::HostError;
use crate::launch::VerifiedWorkerImages;

use super::super::PairCreate;

pub(in crate::launch::bootstrap) fn create_spec_sha256(
    spec: &PairCreate,
) -> Result<String, HostError> {
    let value =
        serde_json::to_value((&spec.options, &spec.config)).map_err(|_| HostError::Identity)?;
    let bytes = serde_json::to_vec(&value).map_err(|_| HostError::Identity)?;
    if bytes.is_empty() || bytes.len() > 64 * 1024 {
        return Err(HostError::Identity);
    }
    Ok(lower_hex(&Sha256::digest(bytes)))
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(HEX[usize::from(byte >> 4)]));
        value.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    value
}

pub(super) fn worker_image(
    images: &VerifiedWorkerImages,
    role: BootstrapContainerRole,
) -> &crate::launch::VerifiedImageIdentity {
    match role {
        BootstrapContainerRole::Runner => images.runner(),
        BootstrapContainerRole::Dind => images.dind(),
    }
}

pub(super) fn require_worker_runtime_id(
    actual: Option<&str>,
    role: BootstrapContainerRole,
    images: &VerifiedWorkerImages,
) -> Result<(), HostError> {
    if actual == Some(worker_image(images, role).runtime_id()) {
        Ok(())
    } else {
        Err(HostError::Ownership)
    }
}
