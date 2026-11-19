// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use crate::error::{WorkflowsError, refuse};
use crate::expression::is_identifier;
use crate::plan::{Container, Service};
use crate::raw::{RawContainer, RawService};

const VALUE_FLAGS: [&str; 13] = [
    "--cpus",
    "--health-cmd",
    "--health-interval",
    "--health-retries",
    "--health-start-interval",
    "--health-start-period",
    "--health-timeout",
    "--hostname",
    "--memory",
    "--shm-size",
    "--stop-timeout",
    "--ulimit",
    "--user",
];
const SWITCH_FLAGS: [&str; 2] = ["--init", "--read-only"];
const HEALTH_CMD: &str = "--health-cmd";

pub(crate) fn container(job: &str, raw: RawContainer) -> Result<Container, WorkflowsError> {
    let place = format!("job {job} container");
    pinned(&place, &raw.image)?;
    let options = raw
        .options
        .map(|text| plain(&place, "options", text))
        .transpose()?;
    if let Some(text) = &options {
        flags(&place, text)?;
    }
    Ok(Container {
        image: raw.image,
        options,
        env: env(&place, raw.env)?,
    })
}

pub(crate) fn services(
    job: &str,
    raw: Vec<RawService>,
    has_container: bool,
) -> Result<Vec<Service>, WorkflowsError> {
    let mut seen: Vec<Service> = Vec::new();
    let mut host_ports: Vec<u16> = Vec::new();
    for service in raw {
        let place = format!("job {job} service {}", service.name);
        if !is_identifier(&service.name, true) || seen.iter().any(|s| s.name == service.name) {
            return refuse(format!("{place} must have a unique identifier as its name"));
        }
        pinned(&place, &service.image)?;
        for port in &service.ports {
            if let Some(host) = port_spec(&place, port, has_container)? {
                if host_ports.contains(&host) {
                    return refuse(format!(
                        "{place} port {port:?} binds host port {host}, which another port of job {job} already binds"
                    ));
                }
                host_ports.push(host);
            }
        }
        let options = plain(&place, "options", service.options.unwrap_or_default())?;
        if !flags(&place, &options)?.contains(&HEALTH_CMD) {
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
            && !name.contains('@')
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

fn port_spec(place: &str, port: &str, has_container: bool) -> Result<Option<u16>, WorkflowsError> {
    let valid = |text: &str| text.parse::<u16>().ok().filter(|number| *number > 0);
    match port.split_once(':') {
        Some((host, container)) => {
            if let (Some(host), Some(_)) = (valid(host), valid(container)) {
                return Ok(Some(host));
            }
        }
        None if valid(port).is_some() => {
            if has_container {
                return Ok(None);
            }
            return refuse(format!(
                "{place} port {port:?} needs a host port, as <host port>:<container port>; without a job container the runner picks a random host port, and no generated step can read it"
            ));
        }
        None => {}
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

/// The flags of a `docker create` option string. Only the flags of `VALUE_FLAGS` and
/// `SWITCH_FLAGS` pass, so no token can be read as the image, a network, an entrypoint or a mount.
fn flags<'a>(place: &str, options: &'a str) -> Result<Vec<&'a str>, WorkflowsError> {
    let refused = |reason: &str| {
        refuse(format!(
            "{place} options {options:?} {reason}; options may hold only {VALUE_FLAGS:?} with a value, written --flag value or --flag=value, and {SWITCH_FLAGS:?}, quoted with double quotes"
        ))
    };
    let Some(tokens) = split(options) else {
        return refused(
            "have a quote that is not a double quote, a backslash or an unclosed quote",
        );
    };
    let mut found = Vec::new();
    let mut tokens = tokens.into_iter();
    while let Some(token) = tokens.next() {
        let (flag, inline) = match token.split_once('=') {
            Some((flag, value)) => (flag, Some(value)),
            None => (token, None),
        };
        if SWITCH_FLAGS.contains(&flag) && inline.is_none() {
            found.push(flag);
        } else if VALUE_FLAGS.contains(&flag) {
            let value = inline.or_else(|| tokens.next());
            if value.is_none_or(|value| value.is_empty() || value.starts_with('-')) {
                return refused(&format!("give {flag} no value"));
            }
            found.push(flag);
        } else {
            return refused(&format!("hold {token:?}, which is not an allowed flag"));
        }
    }
    Ok(found)
}

/// Splits on blanks outside double quotes. `None` for a single quote or backslash, which the
/// runner and a shell read differently, and for an unclosed double quote.
fn split(options: &str) -> Option<Vec<&str>> {
    if options.contains(['\'', '\\']) {
        return None;
    }
    let mut tokens = Vec::new();
    let mut start = None;
    let mut quoted = false;
    for (index, c) in options.char_indices() {
        match c {
            '"' => {
                quoted = !quoted;
                start.get_or_insert(index);
            }
            c if c.is_whitespace() && !quoted => {
                if let Some(from) = start.take() {
                    tokens.push(unquoted(&options[from..index]));
                }
            }
            _ => {
                start.get_or_insert(index);
            }
        }
    }
    if quoted {
        return None;
    }
    if let Some(from) = start {
        tokens.push(unquoted(&options[from..]));
    }
    Some(tokens)
}

fn unquoted(token: &str) -> &str {
    token
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(token)
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
