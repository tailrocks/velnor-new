use super::super::{
    OciCandidateError, Result,
    archive::{OciArchive, read_blob},
    layer::LayerDescriptor,
};
use sha2::Digest;

pub(super) struct Descriptor {
    pub(super) digest: String,
    pub(super) size: u64,
    pub(super) media_type: String,
}

impl From<Descriptor> for LayerDescriptor {
    fn from(value: Descriptor) -> Self {
        Self {
            digest: value.digest,
            size: value.size,
            media_type: value.media_type,
        }
    }
}

pub(super) fn descriptor(value: &serde_json::Value, strict: bool) -> Result<Descriptor> {
    let object = value
        .as_object()
        .ok_or(OciCandidateError("oci_descriptor_shape"))?;
    if !["mediaType", "digest", "size"]
        .iter()
        .all(|key| object.contains_key(*key))
    {
        return Err(OciCandidateError("oci_descriptor_required"));
    }
    if strict {
        reject_unknown(
            object,
            &["mediaType", "digest", "size", "platform", "annotations"],
        )?;
    }
    let digest = object
        .get("digest")
        .and_then(serde_json::Value::as_str)
        .and_then(|value| value.strip_prefix("sha256:"))
        .filter(|value| is_digest(value))
        .ok_or(OciCandidateError("oci_descriptor_digest"))?
        .to_owned();
    let size = object
        .get("size")
        .and_then(serde_json::Value::as_u64)
        .filter(|size| *size > 0)
        .ok_or(OciCandidateError("oci_descriptor_size"))?;
    let media_type = object
        .get("mediaType")
        .and_then(serde_json::Value::as_str)
        .ok_or(OciCandidateError("oci_descriptor_media_type"))?
        .to_owned();
    Ok(Descriptor {
        digest,
        size,
        media_type,
    })
}

pub(super) fn read_referenced(
    archive: &mut OciArchive,
    descriptor: &Descriptor,
) -> Result<Vec<u8>> {
    let blob = archive
        .blobs
        .get(&descriptor.digest)
        .ok_or(OciCandidateError("oci_blob_missing"))?;
    if blob.size != descriptor.size {
        return Err(OciCandidateError("oci_blob_size"));
    }
    let bytes = read_blob(&mut archive.file, blob, 64 * 1024 * 1024)?;
    if sha256(&bytes) != descriptor.digest {
        return Err(OciCandidateError("oci_blob_digest"));
    }
    Ok(bytes)
}

pub(super) fn add_reference(
    references: &mut std::collections::BTreeMap<String, (u64, String)>,
    digest: &str,
    size: u64,
    media_type: &str,
) -> Result<()> {
    let value = (size, media_type.to_owned());
    if references
        .get(digest)
        .is_some_and(|existing| existing != &value)
    {
        return Err(OciCandidateError("oci_descriptor_conflict"));
    }
    references.insert(digest.to_owned(), value);
    Ok(())
}

pub(super) fn reject_unknown(
    object: &serde_json::Map<String, serde_json::Value>,
    allowed: &[&str],
) -> Result<()> {
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(OciCandidateError("oci_unknown_field"));
    }
    Ok(())
}

pub(super) fn is_layer_media_type(value: &str) -> bool {
    matches!(
        value,
        "application/vnd.oci.image.layer.v1.tar"
            | "application/vnd.oci.image.layer.v1.tar+gzip"
            | "application/vnd.oci.image.layer.nondistributable.v1.tar"
            | "application/vnd.oci.image.layer.nondistributable.v1.tar+gzip"
    )
}

fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let digest = sha2::Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}
