// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_model::{
    ImageConfig, ImageCredential, ImageHost, ImageOpenssl, ImagePrBuild, ImageRegistry,
    ImageVisibility,
};

use crate::config::TIMEOUT_RANGE;
use crate::error::Reason;

const MAX_IMAGES: usize = 4;
const MAX_BINS: usize = 8;
const MAX_PR_ONLY: usize = 8;
const MAX_REPOSITORY_BYTES: usize = 255;
const DEFAULT_TIMEOUT: u16 = 45;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawImage {
    name: String,
    bins: Vec<String>,
    package: Option<String>,
    root: Option<String>,
    #[serde(default)]
    features: Vec<String>,
    default_features: Option<bool>,
    profile: Option<String>,
    dockerfile: Option<String>,
    context: Option<String>,
    stage: Option<String>,
    #[serde(default)]
    inputs: Vec<String>,
    glibc: Option<String>,
    openssl: Option<String>,
    pr_build: Option<String>,
    attestations: Option<bool>,
    #[serde(default)]
    pr_only_checks: Vec<String>,
    timeout_minutes: Option<u16>,
    #[serde(default)]
    registry: Vec<RawRegistry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawRegistry {
    host: String,
    repository: String,
    username: Option<String>,
    token: Option<String>,
    visibility: Option<String>,
}

fn invalid(key: &'static str, value: &str) -> Reason {
    Reason::InvalidValue {
        key,
        value: value.to_owned(),
    }
}

pub(crate) fn parse(raw: Vec<RawImage>) -> Result<Vec<ImageConfig>, Reason> {
    if raw.len() > MAX_IMAGES {
        return Err(invalid("image", "more than 4 entries"));
    }
    let mut images: Vec<ImageConfig> = Vec::new();
    for entry in raw {
        let image = image(entry)?;
        if images.iter().any(|other| other.name == image.name) {
            return Err(invalid("image.name", &image.name));
        }
        images.push(image);
    }
    Ok(images)
}

fn image(raw: RawImage) -> Result<ImageConfig, Reason> {
    if !name(&raw.name) {
        return Err(invalid("image.name", &raw.name));
    }
    let context = path(raw.context.as_deref().unwrap_or("."), true, "image.context")?;
    let stage = path(raw.stage.as_deref().unwrap_or("dist"), false, "image.stage")?;
    let mut inputs = Vec::new();
    for input in &raw.inputs {
        let input = path(input, false, "image.inputs")?;
        if !under(&context, &input) || inputs.contains(&input) {
            return Err(invalid("image.inputs", &input));
        }
        inputs.push(input);
    }
    Ok(ImageConfig {
        bins: bins(&raw.bins)?,
        package: optional_word(raw.package, "image.package")?,
        root: raw
            .root
            .map(|root| path(&root, true, "image.root"))
            .transpose()?,
        features: features(&raw.features)?,
        default_features: raw.default_features.unwrap_or(true),
        profile: profile(raw.profile)?,
        dockerfile: path(
            raw.dockerfile.as_deref().unwrap_or("Dockerfile"),
            false,
            "image.dockerfile",
        )?,
        glibc: glibc(raw.glibc)?,
        openssl: openssl(raw.openssl.as_deref())?,
        pr_build: pr_build(raw.pr_build.as_deref())?,
        attestations: raw.attestations.unwrap_or(true),
        pr_only_checks: pr_only_checks(&raw.pr_only_checks)?,
        timeout_minutes: timeout(raw.timeout_minutes)?,
        registries: registries(raw.registry)?,
        name: raw.name,
        context,
        stage,
        inputs,
    })
}

fn pr_only_checks(raw: &[String]) -> Result<Vec<String>, Reason> {
    if raw.len() > MAX_PR_ONLY {
        return Err(invalid("image.pr_only_checks", "more than 8 entries"));
    }
    let mut checks: Vec<String> = Vec::new();
    for check in raw {
        let plain = !check.trim().is_empty()
            && check.len() <= 100
            && check.trim() == check
            && !check.chars().any(char::is_control);
        if !plain || checks.contains(check) {
            return Err(invalid("image.pr_only_checks", check));
        }
        checks.push(check.clone());
    }
    Ok(checks)
}

fn name(text: &str) -> bool {
    let bytes = text.as_bytes();
    (2..=40).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[bytes.len() - 1] != b'-'
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn word(text: &str) -> bool {
    (1..=64).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn optional_word(value: Option<String>, key: &'static str) -> Result<Option<String>, Reason> {
    match value {
        Some(text) if !word(&text) => Err(invalid(key, &text)),
        other => Ok(other),
    }
}

fn bins(raw: &[String]) -> Result<Vec<String>, Reason> {
    if raw.is_empty() || raw.len() > MAX_BINS {
        return Err(invalid("image.bins", &format!("{} entries", raw.len())));
    }
    let mut bins: Vec<String> = Vec::new();
    for bin in raw {
        if !word(bin) || bins.contains(bin) {
            return Err(invalid("image.bins", bin));
        }
        bins.push(bin.clone());
    }
    Ok(bins)
}

fn features(raw: &[String]) -> Result<Vec<String>, Reason> {
    let mut features: Vec<String> = Vec::new();
    for feature in raw {
        let plain = !feature.is_empty()
            && feature.bytes().all(|b| {
                b.is_ascii_alphanumeric() || matches!(b, b'_' | b'+' | b'.' | b'/' | b'-')
            });
        if !plain || features.contains(feature) {
            return Err(invalid("image.features", feature));
        }
        features.push(feature.clone());
    }
    Ok(features)
}

fn profile(raw: Option<String>) -> Result<String, Reason> {
    let profile = raw.unwrap_or_else(|| "release".to_owned());
    let bytes = profile.as_bytes();
    let plain = bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'));
    if plain && !matches!(profile.as_str(), "test" | "bench") {
        Ok(profile)
    } else {
        Err(invalid("image.profile", &profile))
    }
}

fn glibc(raw: Option<String>) -> Result<String, Reason> {
    let glibc = raw.unwrap_or_else(|| "2.41".to_owned());
    let minor = glibc
        .strip_prefix("2.")
        .filter(|minor| minor.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|minor| minor.parse::<u32>().ok())
        .filter(|minor| (17..=99).contains(minor) && !glibc.contains('+'));
    if minor.is_some() && !glibc.starts_with("2.0") {
        Ok(glibc)
    } else {
        Err(invalid("image.glibc", &glibc))
    }
}

fn openssl(raw: Option<&str>) -> Result<ImageOpenssl, Reason> {
    match raw {
        None | Some("none") => Ok(ImageOpenssl::None),
        Some("sysroot") => Ok(ImageOpenssl::Sysroot),
        Some("vendored") => Ok(ImageOpenssl::Vendored),
        Some(other) => Err(invalid("image.openssl", other)),
    }
}

fn pr_build(raw: Option<&str>) -> Result<ImagePrBuild, Reason> {
    match raw {
        None | Some("inputs") => Ok(ImagePrBuild::Inputs),
        Some("always") => Ok(ImagePrBuild::Always),
        Some("off") => Ok(ImagePrBuild::Off),
        Some(other) => Err(invalid("image.pr_build", other)),
    }
}

fn timeout(raw: Option<u16>) -> Result<u16, Reason> {
    let minutes = raw.unwrap_or(DEFAULT_TIMEOUT);
    if TIMEOUT_RANGE.contains(&minutes) {
        Ok(minutes)
    } else {
        Err(Reason::OutOfRange {
            key: "image.timeout_minutes",
            value: minutes,
            min: *TIMEOUT_RANGE.start(),
            max: *TIMEOUT_RANGE.end(),
        })
    }
}

fn path(text: &str, dot: bool, key: &'static str) -> Result<String, Reason> {
    if dot && text == "." {
        return Ok(text.to_owned());
    }
    let plain = !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'/' | b'-'))
        && !text.starts_with('/')
        && !text.ends_with('/')
        && text
            .split('/')
            .all(|part| !matches!(part, "" | "." | ".." | ".git"));
    if plain {
        Ok(text.to_owned())
    } else {
        Err(invalid(key, text))
    }
}

fn under(context: &str, path: &str) -> bool {
    context == "."
        || path
            .strip_prefix(context)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn registries(raw: Vec<RawRegistry>) -> Result<Vec<ImageRegistry>, Reason> {
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
