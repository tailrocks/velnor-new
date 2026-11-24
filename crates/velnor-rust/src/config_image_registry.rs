// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::{ImageCredential, ImageHost, ImageRegistry, ImageVisibility};

use crate::config_image::invalid;
use crate::error::Reason;

const MAX_REPOSITORY_BYTES: usize = 255;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawRegistry {
    host: String,
    repository: String,
    username: Option<String>,
    token: Option<String>,
    visibility: Option<String>,
}

pub(crate) fn registries(raw: Vec<RawRegistry>) -> Result<Vec<ImageRegistry>, Reason> {
    if raw.is_empty() || raw.len() > 2 {
        return Err(invalid("image.registry", &format!("{} entries", raw.len())));
    }
    let mut registries: Vec<ImageRegistry> = Vec::new();
    for entry in raw {
        let registry = registry(entry)?;
        if registries.iter().any(|other| other.host == registry.host) {
            return Err(invalid("image.registry.host", registry.host.name()));
        }
        registries.push(registry);
    }
    Ok(registries)
}

fn registry(raw: RawRegistry) -> Result<ImageRegistry, Reason> {
    let host = match raw.host.as_str() {
        "docker.io" => ImageHost::DockerHub,
        "ghcr.io" => ImageHost::Ghcr,
        other => return Err(invalid("image.registry.host", other)),
    };
    if !repository(&raw.repository) {
        return Err(invalid("image.registry.repository", &raw.repository));
    }
    let credentials = match host {
        ImageHost::DockerHub => {
            let username = raw
                .username
                .as_deref()
                .and_then(|value| credential(value, &["secrets", "vars"]))
                .ok_or_else(|| invalid("image.registry.username", "secrets.NAME or vars.NAME"))?;
            let token = raw
                .token
                .as_deref()
                .and_then(|value| credential(value, &["secrets"]))
                .ok_or_else(|| invalid("image.registry.token", "secrets.NAME"))?;
            (Some(username), Some(token))
        }
        ImageHost::Ghcr => {
            if raw.username.is_some() || raw.visibility.is_some() {
                return Err(invalid(
                    "image.registry.username",
                    "ghcr.io uses github.actor",
                ));
            }
            if raw.token.is_some() {
                return Err(invalid("image.registry.token", "ghcr.io uses github.token"));
            }
            (None, None)
        }
    };
    Ok(ImageRegistry {
        host,
        repository: raw.repository,
        username: credentials.0,
        token: credentials.1,
        visibility: visibility(raw.visibility.as_deref())?,
    })
}

fn visibility(raw: Option<&str>) -> Result<ImageVisibility, Reason> {
    match raw {
        None | Some("private") => Ok(ImageVisibility::Private),
        Some("public") => Ok(ImageVisibility::Public),
        Some(other) => Err(invalid("image.registry.visibility", other)),
    }
}

fn repository(text: &str) -> bool {
    let segment = |part: &str| {
        let bytes = part.as_bytes();
        !bytes.is_empty()
            && bytes
                .first()
                .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            && bytes
                .last()
                .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            && bytes.iter().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
            })
            && !part.contains("..")
            && !part.contains("__")
            && !part.contains("--")
            && !part.contains("._")
            && !part.contains("_.")
            && !part.contains(".-")
            && !part.contains("-.")
            && !part.contains("_-")
            && !part.contains("-_")
    };
    text.len() <= MAX_REPOSITORY_BYTES
        && text
            .split_once('/')
            .is_some_and(|(owner, name)| segment(owner) && segment(name))
}

fn credential(text: &str, contexts: &[&str]) -> Option<ImageCredential> {
    let (context, name) = text.split_once('.')?;
    (contexts.contains(&context)
        && !name.to_ascii_uppercase().starts_with("GITHUB_")
        && !name.is_empty()
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !name.as_bytes()[0].is_ascii_digit())
    .then(|| ImageCredential {
        context: context.to_owned(),
        name: name.to_owned(),
    })
}
