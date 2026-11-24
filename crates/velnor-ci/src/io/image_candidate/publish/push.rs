// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io::{self, Read};

use super::registry::{MANIFEST_TYPE, Registry, fail};
use super::transport::Body;
use crate::error::CiError;
use crate::image_candidate::hashing::sha256_hex;
use crate::image_publish::digest::Digest;
use crate::image_publish::location::{self, Kind};

pub type OpenBlob<'s> = dyn Fn() -> io::Result<Box<dyn Read + Send + 's>> + Sync + 's;

impl Registry<'_> {
    pub fn push_blob(
        &self,
        digest: &Digest,
        size: u64,
        open: &OpenBlob<'_>,
    ) -> Result<(), CiError> {
        if self.exists(Kind::Blobs, digest, size)? {
            return Ok(());
        }
        let started = self.request("POST", location::uploads(), Vec::new(), Body::Bytes(&[]))?;
        let target = started.header("location").map(str::to_owned);
        let Some(target) = target.filter(|_| started.status == 202) else {
            return fail("Registry upload session could not start.");
        };
        let url = location::with_digest(&location::upload(&target)?, digest)?;
        let reader = open().map_err(|_| CiError::new("Cannot read OCI blob from archive."))?;
        let sent = self.request(
            "PUT",
            url,
            vec![("Content-Type", "application/octet-stream".to_owned())],
            Body::Stream {
                reader,
                length: size,
            },
        );
        let response = match sent {
            Ok(response) => response,
            Err(error) if self.exists(Kind::Blobs, digest, size)? => {
                drop(error);
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        if response.status != 201 {
            return fail("Registry blob upload did not complete.");
        }
        if !self.exists(Kind::Blobs, digest, size)? {
            return fail("Registry blob is absent after upload.");
        }
        Ok(())
    }

    pub fn push_manifest(&self, reference: &str, manifest: &[u8]) -> Result<(), CiError> {
        let digest = Digest::parse(reference)?;
        if sha256_hex(manifest) != digest.hex() {
            return fail("OCI manifest bytes are not the bytes identified by its digest.");
        }
        let size = manifest.len() as u64;
        if self.exists(Kind::Manifests, &digest, size)? {
            return self.verify(Kind::Manifests, &digest, size);
        }
        let sent = self.request(
            "PUT",
            location::object(Kind::Manifests, &digest),
            vec![("Content-Type", MANIFEST_TYPE.to_owned())],
            Body::Bytes(manifest),
        );
        let response = match sent {
            Ok(response) => response,
            Err(error) if self.exists(Kind::Manifests, &digest, size)? => {
                drop(error);
                return self.verify(Kind::Manifests, &digest, size);
            }
            Err(error) => return Err(error),
        };
        if response.status != 201 {
            return fail("Registry manifest upload did not complete.");
        }
        if response
            .header("docker-content-digest")
            .is_some_and(|header| header != digest.as_str())
        {
            return fail(
                "Registry response has a manifest digest that is different from the requested digest.",
            );
        }
        if !self.exists(Kind::Manifests, &digest, size)? {
            return fail("Registry manifest is absent after upload.");
        }
        self.verify(Kind::Manifests, &digest, size)
    }
}
