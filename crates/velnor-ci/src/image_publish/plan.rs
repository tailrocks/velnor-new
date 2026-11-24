// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;

use super::digest::Digest;
use crate::error::CiError;
use crate::image_candidate::archive::MAX_BLOB_BYTES;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blob {
    pub digest: Digest,
    pub size: u64,
}

fn fail<T>(message: &str) -> Result<T, CiError> {
    Err(CiError::new(message))
}

pub fn blobs(manifest: &Value) -> Result<Vec<Blob>, CiError> {
    let (Some(layers), true) = (
        manifest["layers"].as_array(),
        manifest["config"].is_object(),
    ) else {
        return fail("OCI manifest blob descriptors are incorrect.");
    };
    let mut unique: Vec<Blob> = Vec::new();
    for descriptor in std::iter::once(&manifest["config"]).chain(layers) {
        let digest = descriptor["digest"].as_str().map(Digest::parse);
        let size = descriptor["size"]
            .as_u64()
            .filter(|size| (1..=MAX_BLOB_BYTES).contains(size));
        let (Some(Ok(digest)), Some(size)) = (digest, size) else {
            return fail("OCI blob descriptor is incorrect.");
        };
        match unique.iter().find(|known| known.digest == digest) {
            Some(known) if known.size != size => {
                return fail("Repeated OCI blob digest has inconsistent sizes.");
            }
            Some(_) => {}
            None => unique.push(Blob { digest, size }),
        }
    }
    Ok(unique)
}
