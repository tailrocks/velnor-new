use sha2::{Digest, Sha256};

use crate::error::HostError;

const MAX_DOCUMENT_BYTES: u64 = 64 * 1024;
const RUNTIME_IDENTITY_PATH: &str = "/.velnor-runtime-identity/runner.json";
const JOB_FACTS_PATH: &str = "/opt/velnor/job-facts/storage-cache-admission.json";

/// Fixed file count and byte limits for the metadata volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ControllerPublicationBound {
    digest: [u8; 32],
}

impl ControllerPublicationBound {
    pub(crate) fn fixed() -> Self {
        Self {
            digest: fixed_policy_digest(),
        }
    }

    pub(crate) fn digest(&self) -> String {
        lower_hex(&self.digest)
    }

    pub(crate) const fn runtime_identity_max_bytes(&self) -> u64 {
        MAX_DOCUMENT_BYTES
    }

    pub(crate) const fn job_facts_max_bytes(&self) -> u64 {
        MAX_DOCUMENT_BYTES
    }

    /// Allow two final files and at most one temporary file.
    pub(crate) const fn peak_payload_bytes(&self) -> u64 {
        MAX_DOCUMENT_BYTES * 3
    }

    pub(crate) const fn final_file_count(&self) -> u64 {
        2
    }

    pub(crate) const fn temporary_file_count(&self) -> u64 {
        1
    }

    pub(crate) const fn directory_count(&self) -> u64 {
        2
    }

    /// One volume root, two directories, two final files and one temporary file.
    pub(crate) const fn object_count(&self) -> u64 {
        6
    }

    pub(crate) fn validate(&self) -> Result<(), HostError> {
        if self.digest != fixed_policy_digest()
            || self.peak_payload_bytes() > i64::MAX as u64
            || self.object_count()
                != 1 + self.final_file_count()
                    + self.temporary_file_count()
                    + self.directory_count()
        {
            return Err(HostError::Journal);
        }
        Ok(())
    }
}

fn fixed_policy_digest() -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"velnor-controller-publication-bound-v1\0");
    for path in [RUNTIME_IDENTITY_PATH, JOB_FACTS_PATH] {
        digest.update((path.len() as u64).to_be_bytes());
        digest.update(path.as_bytes());
    }
    digest.update(MAX_DOCUMENT_BYTES.to_be_bytes());
    digest.update(3_u64.to_be_bytes());
    digest.finalize().into()
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_publication_bound_matches_reader_limits_and_overlap() {
        let bound = ControllerPublicationBound::fixed();
        assert_eq!(bound.runtime_identity_max_bytes(), 65_536);
        assert_eq!(bound.job_facts_max_bytes(), 65_536);
        assert_eq!(bound.peak_payload_bytes(), 196_608);
        assert_eq!(bound.final_file_count(), 2);
        assert_eq!(bound.temporary_file_count(), 1);
        assert_eq!(bound.directory_count(), 2);
        assert_eq!(bound.object_count(), 6);
        assert!(bound.validate().is_ok());
    }
}
