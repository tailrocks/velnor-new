// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::path::Path;

use velnor_actions_contract::{
    ActionPin, GeneratorBinary, GeneratorLock, LockedGenerator, MiseBootstrap, ReleaseManifest,
};

use super::{MISE_VERSION, PinnedTool, ToolCatalog};

pub use super::lock_verify::{
    ApprovedGeneratorRelease, ApprovedMiseRuntime, catalog_for_approved_generator,
    catalog_for_published_verdict, mise_binary_for_approved_generator, published_verdict_manifest,
    verify_lock_against_approved_release, verify_lock_against_manifest,
};

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LockError {
    Io { path: String, message: String },
    MalformedToml { problem: String },
    Invalid { problem: String },
    Mismatch { problem: String },
}

impl Display for LockError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Io { path, message } => write!(f, "lock_io:{path}:{message}"),
            Self::MalformedToml { problem } => write!(f, "malformed_lock_toml:{problem}"),
            Self::Invalid { problem } => write!(f, "invalid_lock:{problem}"),
            Self::Mismatch { problem } => write!(f, "lock_mismatch:{problem}"),
        }
    }
}

impl std::error::Error for LockError {}

pub fn load_text(path: &Path) -> Result<String, LockError> {
    std::fs::read_to_string(path).map_err(|err| LockError::Io {
        path: path.display().to_string(),
        message: err.to_string(),
    })
}

pub fn parse_generator_lock(text: &str) -> Result<GeneratorLock, LockError> {
    let doc = parse_toml(text)?;
    let lock = lock_from_doc(&doc)?;
    lock.validate("generator.lock")
        .map_err(|err| invalid(err.to_string()))?;
    lock.check_supported_targets("generator.lock")
        .map_err(|err| invalid(err.to_string()))?;
    Ok(lock)
}

pub fn parse_release_manifest(text: &str) -> Result<ReleaseManifest, LockError> {
    let manifest = ReleaseManifest::parse_json(text, "release-manifest.json").map_err(|err| {
        LockError::Invalid {
            problem: err.to_string(),
        }
    })?;
    manifest
        .validate("release-manifest.json")
        .map_err(|err| LockError::Invalid {
            problem: err.to_string(),
        })?;
    Ok(manifest)
}

pub fn verify_version_policy(text: &str, catalog: &ToolCatalog) -> Result<(), LockError> {
    let doc: toml::Table =
        toml::from_str(text).map_err(|err| malformed(err.message().to_owned()))?;
    if doc.get("schema").and_then(toml::Value::as_integer) != Some(1) {
        return Err(invalid("bad_schema".to_owned()));
    }
    let tools = doc
        .get("tools")
        .and_then(toml::Value::as_table)
        .ok_or_else(|| invalid("missing_section:tools".to_owned()))?;
    let pins = PinnedTool::ALL
        .into_iter()
        .map(|tool| (tool.tool_name(), catalog.version(tool)))
        .chain([("mise", MISE_VERSION)]);
    for (name, pinned) in pins {
        match tools.get(name) {
            Some(toml::Value::String(found)) if found == pinned => {}
            Some(found) => {
                let found = found
                    .as_str()
                    .map_or_else(|| found.to_string(), str::to_owned);
                return Err(mismatch(format!("tool:{name}:{found}:{pinned}")));
            }
            None => return Err(mismatch(format!("tool_missing:{name}"))),
        }
    }
    Ok(())
}

pub(crate) fn mismatch(problem: String) -> LockError {
    LockError::Mismatch { problem }
}

fn invalid(problem: String) -> LockError {
    LockError::Invalid { problem }
}

type TomlDoc = Vec<(String, BTreeMap<String, String>)>;

fn parse_toml(text: &str) -> Result<TomlDoc, LockError> {
    let mut doc: TomlDoc = vec![(String::new(), BTreeMap::new())];
    for (index, raw) in text.lines().enumerate() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(name) = line.strip_prefix("[[")
            && let Some(name) = name.strip_suffix("]]")
        {
            doc.push((name.trim().to_owned(), BTreeMap::new()));
        } else if let Some(name) = line.strip_prefix('[')
            && let Some(name) = name.strip_suffix(']')
        {
            doc.push((name.trim().to_owned(), BTreeMap::new()));
        } else if let Some((key, value)) = line.split_once('=') {
            let Some((name, entries)) = doc.last_mut() else {
                return Err(malformed(format!("line:{}:no_section", index + 1)));
            };
            if entries
                .insert(key.trim().to_owned(), parse_value(value.trim())?)
                .is_some()
            {
                return Err(malformed(format!(
                    "line:{}:duplicate_key:{key}:{name}",
                    index + 1
                )));
            }
        } else {
            return Err(malformed(format!("line:{}:bad_line", index + 1)));
        }
    }
    Ok(doc)
}

fn strip_comment(line: &str) -> &str {
    let mut quoted = false;
    let mut escape = false;
    for (index, byte) in line.bytes().enumerate() {
        if escape {
            escape = false;
        } else if byte == b'\\' && quoted {
            escape = true;
        } else if byte == b'"' {
            quoted = !quoted;
        } else if byte == b'#' && !quoted {
            return line[..index].trim_end();
        }
    }
    line
}

fn parse_value(text: &str) -> Result<String, LockError> {
    if let Some(inner) = text.strip_prefix('"')
        && let Some(inner) = inner.strip_suffix('"')
    {
        return unescape(inner);
    }
    if text.starts_with('[') && text.ends_with(']') {
        return Ok(text.to_owned());
    }
    if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(text.to_owned());
    }
    Err(malformed(format!("bad_value:{text}")))
}

fn unescape(text: &str) -> Result<String, LockError> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(char) = chars.next() {
        if char == '\\' {
            match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                other => return Err(malformed(format!("bad_escape:{other:?}"))),
            }
        } else {
            out.push(char);
        }
    }
    Ok(out)
}

fn malformed(problem: String) -> LockError {
    LockError::MalformedToml { problem }
}

fn check_schema(doc: &TomlDoc, want: u32) -> Result<(), LockError> {
    section(doc, "")?
        .get("schema")
        .is_some_and(|schema| schema == &want.to_string())
        .then_some(())
        .ok_or_else(|| LockError::Invalid {
            problem: "bad_schema".to_owned(),
        })
}

fn entry(entries: &BTreeMap<String, String>, key: &str) -> Result<String, LockError> {
    entries
        .get(key)
        .cloned()
        .ok_or_else(|| invalid(format!("missing_key:{key}")))
}

fn section(doc: &TomlDoc, name: &str) -> Result<BTreeMap<String, String>, LockError> {
    doc.iter()
        .find(|(title, _)| title == name)
        .map(|(_, entries)| entries.clone())
        .ok_or_else(|| invalid(format!("missing_section:{name}")))
}

fn lock_from_doc(doc: &TomlDoc) -> Result<GeneratorLock, LockError> {
    check_schema(doc, GeneratorLock::SCHEMA)?;
    let generator = section(doc, "generator")?;
    let mut binaries = Vec::new();
    for (title, entries) in doc {
        if title == "generator.binaries" {
            binaries.push(GeneratorBinary {
                target: entry(entries, "target")?,
                artifact: entry(entries, "artifact")?,
                sha256: entry(entries, "sha256")?,
            });
        }
    }
    let bootstrap = section(doc, "mise-bootstrap")?;
    let mut actions = Vec::new();
    for (title, entries) in doc {
        if title == "actions" {
            actions.push(ActionPin {
                name: entry(entries, "name")?,
                version: entry(entries, "version")?,
                sha: entry(entries, "sha")?,
                reviewed: entry(entries, "reviewed")?,
            });
        }
    }
    Ok(GeneratorLock {
        schema: GeneratorLock::SCHEMA,
        generator: LockedGenerator {
            binary: entry(&generator, "binary")?,
            version: entry(&generator, "version")?,
            commit: entry(&generator, "commit")?,
            binaries,
        },
        mise_bootstrap: MiseBootstrap {
            version: entry(&bootstrap, "version")?,
            artifact: entry(&bootstrap, "artifact")?,
            sha256: entry(&bootstrap, "sha256")?,
        },
        actions,
        source_preview: parse_source_preview(doc, "planner-preview")?,
        staged_source_preview: parse_source_preview(doc, "planner-preview-staged")?,
    })
}

fn parse_source_preview(
    doc: &TomlDoc,
    section: &str,
) -> Result<Option<velnor_actions_contract::source_preview::SourcePreviewLock>, LockError> {
    use velnor_actions_contract::source_preview::{
        SourcePreviewLock, SourcePreviewPayload, SourcePreviewReceipt, SourcePreviewRun,
    };
    let mut sections = doc.iter().filter(|(name, _)| name == section);
    let Some((_, values)) = sections.next() else {
        return Ok(None);
    };
    if sections.next().is_some() || values.len() != 27 {
        return Err(invalid("invalid_planner_preview_section".to_owned()));
    }
    let number = |key: &str| -> Result<u64, LockError> {
        entry(values, key)?
            .parse()
            .map_err(|_| invalid(format!("invalid_planner_preview_number:{key}")))
    };
    let run = |prefix: &str| -> Result<SourcePreviewRun, LockError> {
        let field = |suffix: &str| format!("{prefix}_{suffix}");
        Ok(SourcePreviewRun {
            workflow_path: entry(values, &field("workflow_path"))?,
            workflow_ref: entry(values, &field("workflow_ref"))?,
            workflow_sha: entry(values, &field("workflow_sha"))?,
            run_id: number(&field("run_id"))?,
            run_attempt: u32::try_from(number(&field("run_attempt"))?)
                .map_err(|_| invalid("invalid_planner_preview_attempt".to_owned()))?,
        })
    };
    let preview = SourcePreviewLock {
        receipt: SourcePreviewReceipt {
            schema: u32::try_from(number("schema")?)
                .map_err(|_| invalid("invalid_planner_preview_schema".to_owned()))?,
            repository: entry(values, "repository")?,
            version: entry(values, "version")?,
            source_sha: entry(values, "source_sha")?,
            tag: entry(values, "tag")?,
            release_id: number("release_id")?,
            signer: run("signer")?,
            ci: run("ci")?,
            qualification: run("qualification")?,
            payload: SourcePreviewPayload {
                target: entry(values, "payload_target")?,
                name: entry(values, "payload_name")?,
                size: number("payload_size")?,
                sha256: entry(values, "payload_sha256")?,
            },
        },
        receipt_sha256: entry(values, "receipt_sha256")?,
        bundle_sha256: entry(values, "bundle_sha256")?,
    };
    preview
        .validate_shape()
        .map_err(|error| invalid(error.to_string()))?;
    Ok(Some(preview))
}
