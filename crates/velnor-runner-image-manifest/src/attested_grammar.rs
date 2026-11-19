// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{
    OciDescriptor, OciGrammarError, UntrustedProvenance, parse_oci_descriptor,
    reject_unknown_oci_fields,
};
use serde_json::Value;

type Result<T> = std::result::Result<T, OciGrammarError>;
const INDEX_MEDIA_TYPE: &str = "application/vnd.oci.image.index.v1+json";
const MANIFEST_MEDIA_TYPE: &str = "application/vnd.oci.image.manifest.v1+json";

pub fn validate_attestation_reference(
    attestation_value: &Value,
    platform: &OciDescriptor,
) -> Result<()> {
    if attestation_value
        .get("annotations")
        .and_then(Value::as_object)
        .is_none_or(|annotations| {
            annotations.len() != 2
                || annotations
                    .get("vnd.docker.reference.type")
                    .and_then(Value::as_str)
                    != Some("attestation-manifest")
                || annotations
                    .get("vnd.docker.reference.digest")
                    .and_then(Value::as_str)
                    != Some(format!("sha256:{}", platform.digest).as_str())
        })
    {
        return Err(OciGrammarError("attested_reference"));
    }
    Ok(())
}

pub fn attested_index_manifests(value: &Value, expected: usize) -> Result<&[Value]> {
    let object = value
        .as_object()
        .ok_or(OciGrammarError("attested_index_shape"))?;
    reject_unknown_oci_fields(
        object,
        &["schemaVersion", "mediaType", "manifests", "annotations"],
    )?;
    if object.get("schemaVersion").and_then(Value::as_u64) != Some(2)
        || object.get("mediaType").and_then(Value::as_str) != Some(INDEX_MEDIA_TYPE)
    {
        return Err(OciGrammarError("attested_index_identity"));
    }
    object
        .get("manifests")
        .and_then(Value::as_array)
        .filter(|items| items.len() == expected)
        .map(Vec::as_slice)
        .ok_or(OciGrammarError("attested_index_count"))
}

pub fn split_attested_roles(manifests: &[Value]) -> Result<(&Value, &Value)> {
    let platform = manifests
        .iter()
        .filter(|item| {
            item.get("platform")
                .and_then(Value::as_object)
                .is_some_and(|p| {
                    p.len() == 2
                        && p.get("os").and_then(Value::as_str) == Some("linux")
                        && p.get("architecture").and_then(Value::as_str) == Some("amd64")
                })
        })
        .collect::<Vec<_>>();
    let attestation = manifests
        .iter()
        .filter(|item| {
            item.get("platform")
                .and_then(Value::as_object)
                .is_some_and(|p| {
                    p.len() == 2
                        && p.get("os").and_then(Value::as_str) == Some("unknown")
                        && p.get("architecture").and_then(Value::as_str) == Some("unknown")
                })
        })
        .collect::<Vec<_>>();
    if platform.len() != 1 || attestation.len() != 1 {
        return Err(OciGrammarError("attested_roles"));
    }
    Ok((platform[0], attestation[0]))
}

fn attestation_config(object: &serde_json::Map<String, Value>) -> Result<OciDescriptor> {
    let config_value = object
        .get("config")
        .ok_or(OciGrammarError("attested_config"))?;
    let config = parse_oci_descriptor(config_value, false)?;
    if config_value.as_object().is_none_or(|v| v.len() != 4)
        || config_value.get("data").and_then(Value::as_str) != Some("e30=")
        || config.media_type != "application/vnd.oci.empty.v1+json"
        || config.size != 2
        || config.digest != "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
    {
        return Err(OciGrammarError("attested_config"));
    }
    Ok(config)
}

pub fn attestation_descriptors(
    value: &Value,
    platform: &OciDescriptor,
) -> Result<(OciDescriptor, OciDescriptor)> {
    let object = value
        .as_object()
        .ok_or(OciGrammarError("attested_manifest_shape"))?;
    reject_unknown_oci_fields(
        object,
        &[
            "schemaVersion",
            "mediaType",
            "artifactType",
            "config",
            "layers",
            "subject",
        ],
    )?;
    if object.get("schemaVersion").and_then(Value::as_u64) != Some(2)
        || object.get("mediaType").and_then(Value::as_str) != Some(MANIFEST_MEDIA_TYPE)
        || object.get("artifactType").and_then(Value::as_str)
            != Some("application/vnd.docker.attestation.manifest.v1+json")
    {
        return Err(OciGrammarError("attested_manifest_identity"));
    }
    let subject_value = object
        .get("subject")
        .ok_or(OciGrammarError("attested_subject"))?;
    let subject = parse_oci_descriptor(subject_value, true)?;
    if subject.digest != platform.digest
        || subject.size != platform.size
        || subject.media_type != platform.media_type
        || subject_value.as_object().is_none_or(|v| v.len() != 3)
    {
        return Err(OciGrammarError("attested_subject"));
    }
    let config = attestation_config(object)?;
    let layers = object
        .get("layers")
        .and_then(Value::as_array)
        .filter(|items| items.len() == 1)
        .ok_or(OciGrammarError("attested_layer_count"))?;
    let statement = parse_oci_descriptor(&layers[0], true)?;
    if statement.media_type != "application/vnd.in-toto+json"
        || layers[0].as_object().is_none_or(|object| object.len() != 4)
        || layers[0]
            .get("annotations")
            .and_then(Value::as_object)
            .is_none_or(|a| {
                a.len() != 1
                    || a.get("in-toto.io/predicate-type").and_then(Value::as_str)
                        != Some("https://slsa.dev/provenance/v1")
            })
    {
        return Err(OciGrammarError("attested_statement"));
    }
    if statement.size > 1024 * 1024 {
        return Err(OciGrammarError("attested_metadata_size"));
    }
    Ok((config, statement))
}

#[derive(Debug)]
pub struct MeasuredEnvelopeDigests<'a> {
    pub buildkit_index_sha256: &'a str,
    pub attestation_manifest_sha256: &'a str,
    pub attestation_config_sha256: &'a str,
    pub statement_sha256: &'a str,
}

pub fn verify_envelope_graph(
    envelope: &UntrustedProvenance,
    measured: &MeasuredEnvelopeDigests<'_>,
) -> Result<OciDescriptor> {
    crate::validate_provenance_envelope_limits(envelope)
        .map_err(|error| OciGrammarError(error.0))?;
    let outer = parse_json(&envelope.original_index)?;
    let outer_descriptors = attested_index_manifests(&outer, 1)?;
    let nested = parse_oci_descriptor(&outer_descriptors[0], true)?;
    if nested.media_type != INDEX_MEDIA_TYPE
        || !matches_measured(
            &nested,
            &envelope.buildkit_index,
            measured.buildkit_index_sha256,
        )
    {
        return Err(OciGrammarError("envelope_nested_index"));
    }
    let index = parse_json(&envelope.buildkit_index)?;
    let manifests = attested_index_manifests(&index, 2)?;
    let (platform_value, attestation_value) = split_attested_roles(manifests)?;
    if *attestation_value != envelope.attestation_descriptor {
        return Err(OciGrammarError("envelope_attestation_descriptor"));
    }
    let platform = parse_oci_descriptor(platform_value, true)?;
    let attestation = parse_oci_descriptor(attestation_value, true)?;
    if platform.media_type != MANIFEST_MEDIA_TYPE
        || attestation.media_type != MANIFEST_MEDIA_TYPE
        || !matches_measured(
            &attestation,
            &envelope.attestation_manifest,
            measured.attestation_manifest_sha256,
        )
    {
        return Err(OciGrammarError("envelope_attestation_manifest"));
    }
    validate_attestation_reference(attestation_value, &platform)?;
    let manifest = parse_json(&envelope.attestation_manifest)?;
    let (config, statement) = attestation_descriptors(&manifest, &platform)?;
    if !matches_measured(
        &config,
        &envelope.attestation_config,
        measured.attestation_config_sha256,
    ) || !matches_measured(&statement, &envelope.statement, measured.statement_sha256)
    {
        return Err(OciGrammarError("envelope_attestation_bytes"));
    }
    parse_json(&envelope.statement)?;
    Ok(platform)
}

fn matches_measured(descriptor: &OciDescriptor, bytes: &[u8], digest: &str) -> bool {
    usize::try_from(descriptor.size).ok() == Some(bytes.len()) && descriptor.digest == digest
}

fn parse_json(bytes: &[u8]) -> Result<Value> {
    crate::strict_json::parse_strict_json_bytes(bytes, bytes.len()).map_err(|error| match error {
        crate::ImageContractError::CanonicalJson(message)
            if message == "malformed_json:trailing_content" =>
        {
            OciGrammarError("oci_json_trailing")
        }
        _ => OciGrammarError("oci_json_invalid"),
    })
}
