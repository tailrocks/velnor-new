//! Shared closed schema for native captured compiler and build-script streams.
use super::*;

/// The native owner whose standard streams are captured by a metadata object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapturedMetadataKind {
    /// Rust compiler or rustdoc output.
    #[serde(rename = "rustc")]
    Rustc,
    /// C or C++ compiler output.
    #[serde(rename = "cc")]
    Cc,
    /// Cargo build-script output.
    #[serde(rename = "build-script")]
    BuildScript,
}

impl CapturedMetadataKind {
    /// Resolve the actual native envelope kind for a prediction's adapter.
    pub fn for_adapter(adapter: &str) -> eyre::Result<Self> {
        match adapter {
            "rustc" | "rustdoc" => Ok(Self::Rustc),
            "cc-path-binding-v1" => Ok(Self::Cc),
            "build-script" => Ok(Self::BuildScript),
            _ => eyre::bail!("unsupported captured metadata adapter: {adapter}"),
        }
    }
}

/// The complete native schema for a captured standard-output/error object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedMetadata {
    /// Schema version, currently one.
    pub version: u8,
    /// The native owner of these streams.
    pub kind: CapturedMetadataKind,
    /// Content address of standard output.
    pub stdout: Digest,
    /// Content address of standard error.
    pub stderr: Digest,
}

impl CapturedMetadata {
    /// Validate the schema version and both complete digest tuples.
    pub fn validate(&self) -> eyre::Result<()> {
        validate_streams(self.version, &self.stdout, &self.stderr)
    }

    /// Decode and validate exactly the canonical owner representation.
    pub fn from_canonical_bytes(bytes: &[u8]) -> eyre::Result<Self> {
        let metadata: Self = serde_json::from_slice(bytes)?;
        metadata.validate()?;
        if canonical_json(&metadata)? != bytes {
            eyre::bail!("captured metadata is not canonical");
        }
        Ok(metadata)
    }
}

pub(super) fn validate_streams(version: u8, stdout: &Digest, stderr: &Digest) -> eyre::Result<()> {
    if version != 1 {
        eyre::bail!("unsupported captured metadata version");
    }
    stdout.validate()?;
    stderr.validate()?;
    Ok(())
}

impl RustcMetadata {
    /// Decode the common canonical envelope and require its rustc owner kind.
    pub fn from_canonical_bytes(bytes: &[u8]) -> eyre::Result<Self> {
        let metadata = CapturedMetadata::from_canonical_bytes(bytes)?;
        if metadata.kind != CapturedMetadataKind::Rustc {
            eyre::bail!("captured metadata is not owned by rustc");
        }
        Ok(Self {
            version: metadata.version,
            kind: "rustc".into(),
            stdout: metadata.stdout,
            stderr: metadata.stderr,
        })
    }
}

impl CcMetadata {
    /// Decode the common canonical envelope and require its cc owner kind.
    pub fn from_canonical_bytes(bytes: &[u8]) -> eyre::Result<Self> {
        let metadata = CapturedMetadata::from_canonical_bytes(bytes)?;
        if metadata.kind != CapturedMetadataKind::Cc {
            eyre::bail!("captured metadata is not owned by cc");
        }
        Ok(Self {
            version: metadata.version,
            kind: "cc".into(),
            stdout: metadata.stdout,
            stderr: metadata.stderr,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata() -> CapturedMetadata {
        CapturedMetadata {
            version: 1,
            kind: CapturedMetadataKind::BuildScript,
            stdout: Digest::blake3(b"out"),
            stderr: Digest::blake3(b"err"),
        }
    }

    #[test]
    fn rejects_foreign_kinds_unknown_fields_and_versions() {
        let metadata = metadata();
        let bytes = canonical_json(&metadata).unwrap();
        assert_eq!(
            CapturedMetadata::from_canonical_bytes(&bytes).unwrap(),
            metadata
        );
        for (field, value) in [
            ("kind", serde_json::json!("foreign")),
            ("version", serde_json::json!(2)),
            ("unknown", serde_json::json!(true)),
        ] {
            let mut value_object = serde_json::to_value(&metadata).unwrap();
            value_object[field] = value;
            assert!(
                CapturedMetadata::from_canonical_bytes(&canonical_json(&value_object).unwrap())
                    .is_err()
            );
        }
    }

    #[test]
    fn rejects_noncanonical_metadata_and_invalid_digest_tuples() {
        let metadata = metadata();
        let mut bytes = canonical_json(&metadata).unwrap();
        bytes.push(b'\n');
        assert!(CapturedMetadata::from_canonical_bytes(&bytes).is_err());
        let mut metadata = metadata;
        metadata.stdout.hash = "bad".into();
        assert!(metadata.validate().is_err());
    }

    #[test]
    fn strong_adapter_codecs_reject_other_supported_owner_kinds() {
        let mut metadata = metadata();
        for kind in [
            CapturedMetadataKind::Rustc,
            CapturedMetadataKind::Cc,
            CapturedMetadataKind::BuildScript,
        ] {
            metadata.kind = kind;
            let bytes = canonical_json(&metadata).unwrap();
            assert!(CapturedMetadata::from_canonical_bytes(&bytes).is_ok());
            assert_eq!(
                RustcMetadata::from_canonical_bytes(&bytes).is_ok(),
                kind == CapturedMetadataKind::Rustc
            );
            assert_eq!(
                CcMetadata::from_canonical_bytes(&bytes).is_ok(),
                kind == CapturedMetadataKind::Cc
            );
        }
    }

    #[test]
    fn maps_only_actual_native_prediction_owners_to_metadata_kinds() {
        for (adapter, kind) in [
            ("rustc", CapturedMetadataKind::Rustc),
            ("rustdoc", CapturedMetadataKind::Rustc),
            ("cc-path-binding-v1", CapturedMetadataKind::Cc),
            ("build-script", CapturedMetadataKind::BuildScript),
        ] {
            assert_eq!(CapturedMetadataKind::for_adapter(adapter).unwrap(), kind);
        }
        assert!(CapturedMetadataKind::for_adapter("foreign").is_err());
        assert!(CapturedMetadataKind::for_adapter("cc").is_err());
    }
}
