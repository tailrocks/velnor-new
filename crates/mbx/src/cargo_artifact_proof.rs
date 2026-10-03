use super::{CargoCaptureReport, CargoCommandBinding};
use sha2::{Digest, Sha256};
use std::process::ExitStatus;

/// Private, in-memory proof. Deserializing a public report never recreates it.
#[derive(Debug)]
pub(super) struct NativeCaptureProof {
    binding: CargoCommandBinding,
    complete: bool,
    _status: ExitStatus,
    fingerprint: Option<[u8; 32]>,
}

impl NativeCaptureProof {
    pub(super) fn seal(report: &CargoCaptureReport, complete: bool, status: ExitStatus) -> Self {
        Self {
            binding: report.command.clone(),
            complete,
            _status: status,
            fingerprint: capture_fingerprint(report),
        }
    }

    fn unchanged(&self, report: &CargoCaptureReport) -> bool {
        self.fingerprint.is_some() && self.fingerprint == capture_fingerprint(report)
    }
}

impl CargoCaptureReport {
    pub fn identity_matches(&self, session_id: &str, root_session_id: &str) -> bool {
        self.native_proof.as_ref().is_some_and(|proof| {
            proof.unchanged(self) && proof.binding.identity_matches(session_id, root_session_id)
        })
    }

    pub fn local_capture_complete(&self) -> bool {
        self.native_proof
            .as_ref()
            .is_some_and(|proof| proof.complete && proof.unchanged(self))
    }
}

fn capture_fingerprint(report: &CargoCaptureReport) -> Option<[u8; 32]> {
    // Public serde excludes secrets; the private digest also binds retained
    // protocol fields, stderr bytes and actual spawn identity against mutation.
    let public = match serde_json::to_vec(report) {
        Ok(bytes) => bytes,
        Err(_) => return None,
    };
    let mut digest = Sha256::new();
    digest.update(b"mbx-native-cargo-capture-proof-v1\0");
    digest.update((public.len() as u64).to_le_bytes());
    digest.update(public);
    for retained in [
        format!("{:?}", report.messages),
        format!("{:?}", report.diagnostics),
        format!("{:?}", report.stderr),
        format!("{:?}", report.command),
        format!("{:?}", report.read_error),
        format!("{:?}", report.forward_error),
    ] {
        digest.update((retained.len() as u64).to_le_bytes());
        digest.update(retained.as_bytes());
    }
    Some(digest.finalize().into())
}
