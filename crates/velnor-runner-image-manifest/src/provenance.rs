// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::{self, Write};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceEnvelopeError(pub &'static str);

type Result<T> = std::result::Result<T, ProvenanceEnvelopeError>;
const METADATA_LIMIT: usize = 1024 * 1024;
const MAX_ENVELOPE_ARCHIVE_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UntrustedProvenance {
    pub original_archive_sha256: String,
    pub original_archive_size_bytes: u64,
    pub original_index: Vec<u8>,
    pub buildkit_index: Vec<u8>,
    pub attestation_descriptor: Value,
    pub attestation_manifest: Vec<u8>,
    pub attestation_config: Vec<u8>,
    pub statement: Vec<u8>,
}

pub fn validate_provenance_envelope_limits(envelope: &UntrustedProvenance) -> Result<()> {
    if envelope.original_archive_sha256.len() != 64
        || !envelope
            .original_archive_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || envelope.original_archive_size_bytes == 0
        || envelope.original_archive_size_bytes > MAX_ENVELOPE_ARCHIVE_BYTES
        || [
            envelope.original_index.len(),
            envelope.buildkit_index.len(),
            envelope.attestation_manifest.len(),
            envelope.statement.len(),
        ]
        .iter()
        .any(|length| *length == 0 || *length > METADATA_LIMIT)
        || envelope.attestation_config != b"{}"
    {
        return Err(ProvenanceEnvelopeError("envelope_limits"));
    }
    Ok(())
}

const SCHEMA: &str = "velnor.runner-provenance-envelope.v1";
const DOCUMENT_LIMIT: usize = 8 * 1024 * 1024;
const COMPONENT_LIMIT: usize = 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema: String,
    original_archive_sha256: String,
    original_archive_size_bytes: u64,
    original_index_b64: String,
    buildkit_index_b64: String,
    attestation_descriptor: Value,
    attestation_manifest_b64: String,
    attestation_config_b64: String,
    statement_b64: String,
}

#[derive(Serialize)]
struct DocumentRef<'a> {
    schema: &'static str,
    original_archive_sha256: &'a str,
    original_archive_size_bytes: u64,
    original_index_b64: String,
    buildkit_index_b64: String,
    attestation_descriptor: &'a Value,
    attestation_manifest_b64: String,
    attestation_config_b64: String,
    statement_b64: String,
}

struct BoundedWriter(Vec<u8>);

impl Write for BoundedWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > DOCUMENT_LIMIT.saturating_sub(self.0.len()) {
            return Err(io::Error::other("envelope size"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub fn encode_provenance_envelope(value: &UntrustedProvenance) -> Result<Vec<u8>> {
    validate_provenance_envelope_limits(value)?;
    let document = DocumentRef {
        schema: SCHEMA,
        original_archive_sha256: &value.original_archive_sha256,
        original_archive_size_bytes: value.original_archive_size_bytes,
        original_index_b64: STANDARD.encode(&value.original_index),
        buildkit_index_b64: STANDARD.encode(&value.buildkit_index),
        attestation_descriptor: &value.attestation_descriptor,
        attestation_manifest_b64: STANDARD.encode(&value.attestation_manifest),
        attestation_config_b64: STANDARD.encode(&value.attestation_config),
        statement_b64: STANDARD.encode(&value.statement),
    };
    let mut output = BoundedWriter(Vec::new());
    serde_json::to_writer(&mut output, &document)
        .map_err(|_| ProvenanceEnvelopeError("envelope_size_or_encode"))?;
    Ok(output.0)
}

pub fn parse_provenance_envelope(bytes: &[u8]) -> Result<UntrustedProvenance> {
    if bytes.len() > DOCUMENT_LIMIT {
        return Err(ProvenanceEnvelopeError("envelope_size"));
    }
    let value = crate::strict_json::parse_strict_json_bytes(bytes, DOCUMENT_LIMIT)
        .map_err(|_| ProvenanceEnvelopeError("envelope_json"))?;
    let document: Document =
        serde_json::from_value(value).map_err(|_| ProvenanceEnvelopeError("envelope_shape"))?;
    if document.schema != SCHEMA {
        return Err(ProvenanceEnvelopeError("envelope_schema"));
    }
    let result = UntrustedProvenance {
        original_archive_sha256: document.original_archive_sha256,
        original_archive_size_bytes: document.original_archive_size_bytes,
        original_index: decode(&document.original_index_b64)?,
        buildkit_index: decode(&document.buildkit_index_b64)?,
        attestation_descriptor: document.attestation_descriptor,
        attestation_manifest: decode(&document.attestation_manifest_b64)?,
        attestation_config: decode(&document.attestation_config_b64)?,
        statement: decode(&document.statement_b64)?,
    };
    validate_provenance_envelope_limits(&result)?;
    if encode_provenance_envelope(&result)? != bytes {
        return Err(ProvenanceEnvelopeError("envelope_noncanonical"));
    }
    Ok(result)
}

fn decode(value: &str) -> Result<Vec<u8>> {
    if value.len() > COMPONENT_LIMIT.div_ceil(3) * 4 {
        return Err(ProvenanceEnvelopeError("envelope_component_size"));
    }
    let bytes = STANDARD
        .decode(value)
        .map_err(|_| ProvenanceEnvelopeError("envelope_base64"))?;
    if bytes.len() > COMPONENT_LIMIT || STANDARD.encode(&bytes) != value {
        return Err(ProvenanceEnvelopeError("envelope_base64"));
    }
    Ok(bytes)
}
