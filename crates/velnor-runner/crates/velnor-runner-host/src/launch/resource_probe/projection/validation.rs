use bollard::models::ContainerConfig;
use sha2::{Digest, Sha256};

use super::{
    PATH_ENV, PROBE_ENTRYPOINT, PROBE_USER, PROBE_WORKING_DIR, ProbeProjection, SOURCE_LABEL,
};

pub(super) fn canonical_absolute_nonroot(path: &str) -> bool {
    path.starts_with('/')
        && path != "/"
        && path.len() <= 4096
        && !path.chars().any(char::is_control)
        && path[1..]
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

pub(super) fn digest_reference(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex_string(hex, 64))
}

pub(super) fn valid_daemon_architecture(value: &str) -> bool {
    matches!(value, "x86_64" | "aarch64")
}

pub(super) fn hex_string(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn digest_text(value: &[u8]) -> String {
    let digest = Sha256::digest(value);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(HEX[(byte >> 4) as usize]));
        encoded.push(char::from(HEX[(byte & 0x0f) as usize]));
    }
    encoded
}

const HEX: &[u8; 16] = b"0123456789abcdef";

pub(super) fn image_config_matches(config: &ContainerConfig, source_revision: &str) -> bool {
    config == &expected_image_config(source_revision)
}

pub(in crate::launch::resource_probe) fn expected_container_config(
    projection: &ProbeProjection,
) -> Option<ContainerConfig> {
    let mut expected =
        serde_json::to_value(expected_image_config(projection.image.source_revision())).ok()?;
    let requested = serde_json::to_value(&projection.config).ok()?;
    let expected = expected.as_object_mut()?;
    let requested = requested.as_object()?;
    for (key, value) in requested {
        if value.is_null() || key == "HostConfig" || key == "NetworkingConfig" {
            continue;
        }
        if key == "Labels" {
            let expected_labels = expected
                .get_mut(key)
                .and_then(serde_json::Value::as_object_mut)?;
            let requested_labels = value.as_object()?;
            for (label, label_value) in requested_labels {
                expected_labels.insert(label.clone(), label_value.clone());
            }
        } else {
            expected.insert(key.clone(), value.clone());
        }
    }
    serde_json::from_value(serde_json::Value::Object(expected.clone())).ok()
}

pub(in crate::launch::resource_probe) fn expected_image_config(
    source_revision: &str,
) -> ContainerConfig {
    let mut labels = std::collections::HashMap::new();
    labels.insert(SOURCE_LABEL.to_owned(), source_revision.to_owned());
    ContainerConfig {
        user: Some(PROBE_USER.to_owned()),
        env: Some(vec![PATH_ENV.to_owned()]),
        working_dir: Some(PROBE_WORKING_DIR.to_owned()),
        entrypoint: Some(vec![PROBE_ENTRYPOINT.to_owned()]),
        labels: Some(labels),
        ..Default::default()
    }
}
