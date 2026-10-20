use serde_json::json;
use sha2::{Digest, Sha256};

pub(super) struct ClaimIdentity {
    pub(super) lineage_id: i64,
    pub(super) generation: i64,
    pub(super) reservation_id: i64,
    pub(super) intent_id: i64,
    pub(super) connection_id: String,
    pub(super) scale_set_id: i64,
    pub(super) witness_request_id: i64,
    pub(super) binding_digest: String,
    pub(super) source_digest: String,
    pub(super) acquire_response_digest: String,
    pub(super) engine_id: String,
    pub(super) docker_root_digest: String,
    pub(super) instance_binding_digest: String,
    pub(super) runner_id: i64,
    pub(super) runner_name: String,
}

impl ClaimIdentity {
    pub(super) fn snapshot_digest(&self) -> Result<String, serde_json::Error> {
        let value = json!([
            "velnor-post-jit-retirement-identity-v1",
            self.lineage_id,
            self.generation,
            self.reservation_id,
            self.intent_id,
            self.connection_id,
            self.scale_set_id,
            self.witness_request_id,
            self.binding_digest,
            self.source_digest,
            self.acquire_response_digest,
            self.engine_id,
            self.docker_root_digest,
            self.instance_binding_digest,
            self.runner_id,
            self.runner_name,
        ]);
        let bytes = serde_json::to_vec(&value)?;
        Ok(hex_digest(&bytes))
    }
}

pub(super) fn hex_digest(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}
