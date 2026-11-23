// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use velnor_model::RepoPath;

use super::refuse;
use crate::error::DetectError;

const TOOLS: [&str; 2] = ["ansible-core", "ansible-lint"];
const INSTALL: &str = "python3 -m pip install --upgrade --only-binary=:all: --require-hashes --no-deps --target \"$RUNNER_TEMP/ansible\" -r \"$RUNNER_TEMP/ansible-tools.txt\"";

struct Pin {
    spec: String,
    hashes: Vec<String>,
}

pub(super) struct Reinstall {
    pub(super) write: String,
    pub(super) install: String,
}

pub(super) fn reinstall(bytes: &[u8], path: &RepoPath) -> Result<Reinstall, DetectError> {
    let pins = pins(bytes, path)?;
    let mut body = String::new();
    for name in TOOLS {
        let pin = pins
            .get(name)
            .ok_or_else(|| refuse(path, format!("requirements.txt does not pin {name}")))?;
        if !embeddable(&pin.spec) {
            return Err(refuse(
                path,
                format!("requirements.txt pins {name} with a value this job cannot quote"),
            ));
        }
        body.push_str(&pin.spec);
        for hash in &pin.hashes {
            body.push_str(" --hash=sha256:");
            body.push_str(hash);
        }
        body.push('\n');
    }
    Ok(Reinstall {
        write: format!(
            "python3 -c 'open(__import__(\"os\").environ[\"RUNNER_TEMP\"] + \"/ansible-tools.txt\", \"w\").write(\"{}\")'",
            python_text(&body),
        ),
        install: INSTALL.to_owned(),
    })
}

fn pins(bytes: &[u8], path: &RepoPath) -> Result<BTreeMap<String, Pin>, DetectError> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| refuse(path, "requirements.txt is not utf-8".to_owned()))?;
    let mut pending = String::new();
    let mut pins = BTreeMap::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if let Some(head) = trimmed.strip_suffix('\\') {
            pending.push_str(head.trim_end());
            pending.push(' ');
            continue;
        }
        pending.push_str(trimmed);
        let joined = std::mem::take(&mut pending);
        let joined = joined.trim();
        if joined.is_empty() || joined.starts_with('#') {
            continue;
        }
        let pin = requirement(joined)
            .map_err(|reason| refuse(path, format!("line {}: {reason}", index + 1)))?;
        let name = pin
            .spec
            .split_once("==")
            .map(|(name, _)| name.to_owned())
            .ok_or_else(|| refuse(path, "requirements.txt has a requirement without a name"))?;
        pins.insert(name, pin);
    }
    if !pending.trim().is_empty() {
        return Err(refuse(
            path,
            "requirements.txt ends with a continuation".to_owned(),
        ));
    }
    Ok(pins)
}

fn requirement(line: &str) -> Result<Pin, String> {
    let mut parts = line.split_whitespace();
    let spec = parts.next().ok_or("empty requirement")?;
    let Some((name, version)) = spec.split_once("==") else {
        return Err(format!("{spec} must be name==version"));
    };
    if name.is_empty()
        || version.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(format!("invalid requirement {spec}"));
    }
    let mut hashes = Vec::new();
    for part in parts {
        let Some(hex) = part.strip_prefix("--hash=sha256:") else {
            return Err(format!("unsupported requirement option {part}"));
        };
        if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err("a hash must be sha256 and 64 hex characters".to_owned());
        }
        hashes.push(hex.to_owned());
    }
    if hashes.is_empty() {
        return Err(format!("{name} has no --hash=sha256"));
    }
    Ok(Pin {
        spec: spec.to_owned(),
        hashes,
    })
}

fn embeddable(spec: &str) -> bool {
    spec.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+' | '!' | '='))
}

fn python_text(body: &str) -> String {
    body.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}
