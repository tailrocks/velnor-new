// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde_json::Value;

use super::{OciCandidateError, Result};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OciImageConfiguration {
    pub labels: Option<BTreeMap<String, String>>,
    pub user: Option<String>,
    pub working_directory: Option<String>,
    pub entrypoint: Option<Vec<String>>,
    pub cmd: Option<Vec<String>>,
    pub env: Option<Vec<String>>,
}

pub(super) fn project(image: &Value) -> Result<OciImageConfiguration> {
    let configuration = match image.get("config") {
        None | Some(Value::Null) => return Ok(OciImageConfiguration::default()),
        Some(Value::Object(configuration)) => configuration,
        Some(_) => return Err(OciCandidateError("oci_image_config_shape")),
    };
    Ok(OciImageConfiguration {
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
        .ok_or(OciCandidateError("oci_image_config_labels"))?;
    object
        .iter()
        .map(|(key, value)| {
            value
                .as_str()
                .map(|value| (key.clone(), value.to_owned()))
                .ok_or(OciCandidateError("oci_image_config_labels"))
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
        .ok_or(OciCandidateError("oci_image_config_string"))
}

fn strings(value: Option<&Value>) -> Result<Option<Vec<String>>> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    value
        .as_array()
        .ok_or(OciCandidateError("oci_image_config_strings"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or(OciCandidateError("oci_image_config_strings"))
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
}
