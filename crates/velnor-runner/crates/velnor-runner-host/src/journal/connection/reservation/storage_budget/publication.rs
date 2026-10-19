use sha2::{Digest, Sha256};

use crate::error::HostError;

const LARGE_DOCUMENT_MAX_BYTES: u64 = 64 * 1024;
const CONTROL_DOCUMENT_MAX_BYTES: u64 = 8 * 1024;
const REQUEST_DOCUMENT_MAX_BYTES: u64 = 16 * 1024;
const RUNTIME_IDENTITY_PATH: &str = "/.velnor-runtime-identity/runner.json";
const JOB_FACTS_PATH: &str = "/opt/velnor/job-facts/storage-cache-admission.json";
const CHALLENGE_PATH: &str = "/opt/velnor/source-proof/control/challenge.json";
const DECISION_PATH: &str = "/opt/velnor/source-proof/control/decision.json";
const REQUEST_PATH: &str = "/opt/velnor/source-proof/inbox/request.json";
const HOME_INBOX_INIT_PATH: &str = "/velnor/init/home/source-proof/inbox";
const HOME_INBOX_VOLUME_SUBPATH: &str = "source-proof/inbox";
const TEMPORARY_PATHS: [&str; 3] = [
    "/opt/velnor/source-proof/control/.challenge.<operation_id>.tmp",
    "/opt/velnor/source-proof/control/.decision.<operation_id>.tmp",
    "/opt/velnor/source-proof/inbox/request.<challenge>.tmp",
];
const METADATA_DIRECTORIES: [&str; 4] = [
    "/.velnor-runtime-identity",
    "/opt/velnor/job-facts",
    "/opt/velnor/source-proof",
    "/opt/velnor/source-proof/control",
];
const HOME_INBOX_DIRECTORIES: [&str; 2] = ["/velnor/init/home/source-proof", HOME_INBOX_INIT_PATH];

/// Fixed publication limits split between the metadata volume and `RunnerHome` inbox.
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
        LARGE_DOCUMENT_MAX_BYTES
    }

    pub(crate) const fn job_facts_max_bytes(&self) -> u64 {
        LARGE_DOCUMENT_MAX_BYTES
    }

    pub(crate) const fn challenge_max_bytes(&self) -> u64 {
        CONTROL_DOCUMENT_MAX_BYTES
    }

    pub(crate) const fn decision_max_bytes(&self) -> u64 {
        CONTROL_DOCUMENT_MAX_BYTES
    }

    pub(crate) const fn request_max_bytes(&self) -> u64 {
        REQUEST_DOCUMENT_MAX_BYTES
    }

    /// Bound four metadata documents and the largest simultaneously retained temporaries.
    pub(crate) const fn peak_payload_bytes(&self) -> u64 {
        self.runtime_identity_max_bytes()
            + self.job_facts_max_bytes()
            + self.challenge_max_bytes()
            + self.decision_max_bytes()
            + LARGE_DOCUMENT_MAX_BYTES
            + CONTROL_DOCUMENT_MAX_BYTES
    }

    pub(crate) const fn final_file_count(&self) -> u64 {
        4
    }

    pub(crate) const fn temporary_file_count(&self) -> u64 {
        2
    }

    pub(crate) const fn directory_count(&self) -> u64 {
        METADATA_DIRECTORIES.len() as u64
    }

    /// Count the volume root, initialized directories, final entries, and temporary entries.
    pub(crate) const fn object_count(&self) -> u64 {
        1 + self.final_file_count() + self.temporary_file_count() + self.directory_count()
    }

    /// The request file and its atomic temporary are charged to the existing `RunnerHome` volume.
    pub(crate) const fn home_inbox_peak_payload_bytes(&self) -> u64 {
        2 * self.request_max_bytes()
    }

    /// Count `RunnerHome`'s root, inbox directories, final request, and atomic temporary entry.
    pub(crate) const fn home_inbox_object_count(&self) -> u64 {
        1 + HOME_INBOX_DIRECTORIES.len() as u64 + 2
    }

    pub(crate) fn validate(&self) -> Result<(), HostError> {
        if self.digest != fixed_policy_digest()
            || self.peak_payload_bytes() > i64::MAX as u64
            || self.home_inbox_peak_payload_bytes() > i64::MAX as u64
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
    digest.update(b"velnor-controller-publication-bound-v3\0");
    digest.update(b"engine-metadata\0");
    for path in [
        RUNTIME_IDENTITY_PATH,
        JOB_FACTS_PATH,
        CHALLENGE_PATH,
        DECISION_PATH,
    ] {
        digest.update((path.len() as u64).to_be_bytes());
        digest.update(path.as_bytes());
    }
    for directory in METADATA_DIRECTORIES {
        digest.update((directory.len() as u64).to_be_bytes());
        digest.update(directory.as_bytes());
    }
    for value in [
        REQUEST_PATH,
        HOME_INBOX_INIT_PATH,
        HOME_INBOX_VOLUME_SUBPATH,
    ] {
        digest.update((value.len() as u64).to_be_bytes());
        digest.update(value.as_bytes());
    }
    digest.update(b"runner-home\0");
    for directory in HOME_INBOX_DIRECTORIES {
        digest.update((directory.len() as u64).to_be_bytes());
        digest.update(directory.as_bytes());
    }
    for path in TEMPORARY_PATHS {
        digest.update((path.len() as u64).to_be_bytes());
        digest.update(path.as_bytes());
    }
    for limit in [
        LARGE_DOCUMENT_MAX_BYTES,
        CONTROL_DOCUMENT_MAX_BYTES,
        REQUEST_DOCUMENT_MAX_BYTES,
    ] {
        digest.update(limit.to_be_bytes());
    }
    digest.update(4_u64.to_be_bytes());
    digest.update(2_u64.to_be_bytes());
    digest.update(1_u64.to_be_bytes());
    digest.update(1_u64.to_be_bytes());
    digest.update(2_u64.to_be_bytes());
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
        assert_eq!(bound.challenge_max_bytes(), 8_192);
        assert_eq!(bound.decision_max_bytes(), 8_192);
        assert_eq!(bound.request_max_bytes(), 16_384);
        assert_eq!(bound.peak_payload_bytes(), 221_184);
        assert_eq!(bound.final_file_count(), 4);
        assert_eq!(bound.temporary_file_count(), 2);
        assert_eq!(bound.directory_count(), 4);
        assert_eq!(bound.object_count(), 11);
        assert_eq!(bound.home_inbox_peak_payload_bytes(), 32_768);
        assert_eq!(bound.home_inbox_object_count(), 5);
        assert_eq!(
            bound.digest(),
            "01e2ed30590d4060486104922e6be6c776b116d977b906edb7c6511a310dccb1"
        );
        assert!(bound.validate().is_ok());
    }
}
