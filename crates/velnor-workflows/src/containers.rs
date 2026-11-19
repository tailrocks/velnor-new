// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::error::{WorkflowsError, refuse};
use crate::expression::is_identifier;
use crate::plan::{Container, Service};
use crate::raw::{RawContainer, RawService};

pub(crate) fn container(job: &str, raw: RawContainer) -> Result<Container, WorkflowsError> {
    let place = format!("job {job} container");
    pinned(&place, &raw.image)?;
    let options = raw
        .options
        .map(|text| plain(&place, "options", text))
        .transpose()?;
    Ok(Container {
        image: raw.image,
        options,
        env: env(&place, raw.env)?,
    })
}

pub(crate) fn services(job: &str, raw: Vec<RawService>) -> Result<Vec<Service>, WorkflowsError> {
    let mut seen: Vec<Service> = Vec::new();
    for service in raw {
        let place = format!("job {job} service {}", service.name);
        if !is_identifier(&service.name, true) || seen.iter().any(|s| s.name == service.name) {
            return refuse(format!("{place} must have a unique identifier as its name"));
        }
        pinned(&place, &service.image)?;
        for port in &service.ports {
            port_spec(&place, port)?;
        }
        let options = plain(&place, "options", service.options.unwrap_or_default())?;
        if !options
            .split_whitespace()
            .any(|word| word == "--health-cmd")
        {
            return refuse(format!(
                "{place} needs a --health-cmd in options, or the job starts before the service is ready"
            ));
        }
        seen.push(Service {
            env: env(&place, service.env)?,
            name: service.name,
            image: service.image,
            ports: service.ports,
            options,
        });
    }
    Ok(seen)
}

fn pinned(place: &str, image: &str) -> Result<(), WorkflowsError> {
    let digest = image.rsplit_once("@sha256:");
    let ok = digest.is_some_and(|(name, digest)| {
        !name.is_empty()
            && !name.contains(char::is_whitespace)
            && !name.contains("${{")
            && digest.len() == 64
            && digest
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    });
    if ok {
        return Ok(());
    }
    refuse(format!(
        "{place} image {image:?} is not pinned by digest; use <name>@sha256:<64 lower-case hex digits>"
    ))
}

fn port_spec(place: &str, port: &str) -> Result<(), WorkflowsError> {
    let valid = |text: &str| text.parse::<u16>().is_ok_and(|number| number > 0);
    let ok = match port.split_once(':') {
        Some((host, container)) => valid(host) && valid(container),
        None => valid(port),
    };
    if ok {
        return Ok(());
    }
    refuse(format!(
        "{place} port {port:?} must be <port> or <host port>:<container port>, each 1 to 65535"
    ))
}

fn plain(place: &str, what: &str, text: String) -> Result<String, WorkflowsError> {
    if text.contains(char::is_control) || text.contains("${{") {
        return refuse(format!(
            "{place} {what} must be one line without expressions"
        ));
    }
    Ok(text)
}

fn env(
    place: &str,
    raw: BTreeMap<String, String>,
) -> Result<Vec<(String, String)>, WorkflowsError> {
    raw.into_iter()
        .map(|(key, value)| {
            if !is_identifier(&key, true) {
                return refuse(format!("{place} env key {key:?} is not an identifier"));
            }
            Ok((key, plain(place, "env values", value)?))
        })
        .collect()
}
