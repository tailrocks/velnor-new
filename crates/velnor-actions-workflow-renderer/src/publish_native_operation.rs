// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{RenderError, yaml::Yaml};
use std::collections::BTreeMap;
use velnor_actions_contract::Step;

pub(super) fn render(
    step: &Step,
    operation: &str,
    values: &BTreeMap<String, String>,
) -> Result<Yaml, RenderError> {
    let invalid = || RenderError::InvalidWorkflow("invalid_native_image_operation".to_owned());
    if values.len() != 4 {
        return Err(invalid());
    }
    let profile = values.get("IMAGE_PROFILE").ok_or_else(invalid)?;
    if !matches!(
        profile.as_str(),
        "job" | "runner" | "dind" | "resource-probe"
    ) {
        return Err(invalid());
    }
    let repository = values.get("IMAGE_REPOSITORY").ok_or_else(invalid)?;
    if repository != &format!("ghcr.io/tailrocks/velnor-{profile}") {
        return Err(invalid());
    }
    let version = |key: &str| -> Result<&str, RenderError> {
        let value = values.get(key).ok_or_else(invalid)?;
        if value.split('.').count() != 3
            || value
                .split('.')
                .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
        {
            return Err(invalid());
        }
        Ok(value)
    };
    let gh = version("GH_VERSION")?;
    let crane = version("CRANE_VERSION")?;
    let (name, script, key) = match operation {
        "native-image-publish-v1" => (
            "Publish authenticated native image",
            include_str!("publish_native_image.sh")
                .replace(
                    "__VELNOR_CI_IMAGE_LOOKUP__",
                    crate::schema2::SDK_CI_IMAGE_LOOKUP_SCRIPT,
                )
                .replace(
                    "__VELNOR_CI_IMAGE_DOWNLOAD__",
                    crate::schema2::SDK_CI_IMAGE_DOWNLOAD_SCRIPT,
                )
                .replace(
                    "__VELNOR_ARTIFACT_EXTRACTOR__",
                    crate::schema2::SDK_CI_IMAGE_ARTIFACT_SCRIPT,
                ),
            "IMAGE_PROFILE",
        ),
        "native-image-promote-v1" => (
            "Promote authenticated native image digest",
            include_str!("publish_native_promote.sh").to_owned(),
            "IMAGE_ID",
        ),
        _ => return Err(invalid()),
    };
    if step.name != name
        || step.condition.is_some()
        || step.role.is_some()
        || step.id.is_some()
        || step.parallel
    {
        return Err(invalid());
    }
    let run = format!(
        "set -euo pipefail\nmise --no-config --no-env --no-hooks install gh@{gh} crane@{crane}\ngh() {{ mise --no-config --no-env --no-hooks exec gh@{gh} -- gh \"$@\"; }}\ncrane() {{ mise --no-config --no-env --no-hooks exec crane@{crane} -- crane \"$@\"; }}\n{script}"
    );
    Ok(Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "env".to_owned(),
            Yaml::Map(vec![
                (
                    "GH_TOKEN".to_owned(),
                    Yaml::str("${{ secrets.GITHUB_TOKEN }}"),
                ),
                (key.to_owned(), Yaml::str(profile)),
                ("IMAGE_REPOSITORY".to_owned(), Yaml::str(repository)),
            ]),
        ),
        ("run".to_owned(), Yaml::str(run)),
    ]))
}
