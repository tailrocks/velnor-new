// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::OciGrammarError;
type Result<T> = std::result::Result<T, OciGrammarError>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OciImageConfiguration {
    pub config_keys: BTreeSet<String>,
    pub labels: Option<BTreeMap<String, String>>,
    pub user: Option<String>,
    pub working_directory: Option<String>,
    pub entrypoint: Option<Vec<String>>,
    pub cmd: Option<Vec<String>>,
    pub env: Option<Vec<String>>,
}

pub fn project_oci_configuration(image: &Value) -> Result<OciImageConfiguration> {
    let configuration = match image.get("config") {
        None | Some(Value::Null) => return Ok(OciImageConfiguration::default()),
        Some(Value::Object(configuration)) => configuration,
        Some(_) => return Err(OciGrammarError("oci_image_config_shape")),
    };
    Ok(OciImageConfiguration {
        config_keys: configuration.keys().cloned().collect(),
        labels: labels(configuration.get("Labels"))?,
        user: string(configuration.get("User"))?,
        working_directory: string(configuration.get("WorkingDir"))?,
        entrypoint: strings(configuration.get("Entrypoint"))?,
        cmd: strings(configuration.get("Cmd"))?,
        env: strings(configuration.get("Env"))?,
    })
}

fn labels(value: Option<&Value>) -> Result<Option<BTreeMap<String, String>>> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let object = value
        .as_object()
        .ok_or(OciGrammarError("oci_image_config_labels"))?;
    object
        .iter()
        .map(|(key, value)| {
            value
                .as_str()
                .map(|value| (key.clone(), value.to_owned()))
                .ok_or(OciGrammarError("oci_image_config_labels"))
        })
        .collect::<Result<BTreeMap<_, _>>>()
        .map(Some)
}

fn string(value: Option<&Value>) -> Result<Option<String>> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    value
        .as_str()
        .map(str::to_owned)
        .map(Some)
        .ok_or(OciGrammarError("oci_image_config_string"))
}

fn strings(value: Option<&Value>) -> Result<Option<Vec<String>>> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    value
        .as_array()
        .ok_or(OciGrammarError("oci_image_config_strings"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or(OciGrammarError("oci_image_config_strings"))
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
}

const PROBE_DEFAULT_ENV: &str = "PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

#[must_use]
pub fn has_image_revision(configuration: &OciImageConfiguration, source_commit: &str) -> bool {
    configuration
        .labels
        .as_ref()
        .and_then(|labels| labels.get("org.opencontainers.image.revision"))
        .is_some_and(|revision| revision == source_commit)
}

#[must_use]
pub fn has_image_kind(configuration: &OciImageConfiguration, expected_kind: &str) -> bool {
    configuration
        .labels
        .as_ref()
        .and_then(|labels| labels.get("io.velnor.image.kind"))
        .is_some_and(|image_kind| image_kind == expected_kind)
}

#[must_use]
pub fn has_probe_profile(configuration: &OciImageConfiguration) -> bool {
    configuration.config_keys.len() == 5
        && ["User", "Env", "Entrypoint", "WorkingDir", "Labels"]
            .iter()
            .all(|key| configuration.config_keys.contains(*key))
        && configuration.labels.as_ref().is_some_and(|labels| {
            labels.len() == 1 && labels.contains_key("org.opencontainers.image.revision")
        })
        && configuration.user.as_deref() == Some("65532:65532")
        && configuration.working_directory.as_deref() == Some("/")
        && configuration.entrypoint.as_ref().is_some_and(|entrypoint| {
            entrypoint.len() == 1 && entrypoint[0] == "/velnor/resource-probe"
        })
        && configuration.cmd.is_none()
        && configuration.env.as_ref().is_some_and(|environment| {
            environment.len() == 1 && environment[0] == PROBE_DEFAULT_ENV
        })
}

#[must_use]
pub fn admits_image_configuration(
    configuration: &OciImageConfiguration,
    kind: crate::ImageKind,
    source_commit: &str,
) -> bool {
    has_image_revision(configuration, source_commit)
        && match kind {
            crate::ImageKind::Runner => has_image_kind(configuration, "runner"),
            crate::ImageKind::Dind => has_image_kind(configuration, "dind"),
            crate::ImageKind::ResourceProbe => has_probe_profile(configuration),
        }
}
