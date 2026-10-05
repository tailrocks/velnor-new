//! Bootstrap lock and version-policy IO plus verification.
//!
//! Parses `.velnor/generator.lock` (TOML subset) and the release-manifest
//! JSON, then verifies the lock against the manifest per supported target
//! (URL+SHA equality, immutable URLs) and the version-policy mirror
//! against the compiled catalog. CI runs this before any bootstrap use.

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::path::Path;

use velnor_actions_contract::{
    ActionPin, GeneratorBinary, GeneratorLock, GithubRunnerImages, LockedGenerator, MiseBootstrap,
    ReleaseManifest, RunnerInventory, VersionPolicy,
};

use super::{MISE_VERSION, PinnedTool, ToolCatalog};

pub use super::lock_verify::verify_lock_against_manifest;

/// Lock, manifest, and version-policy verification failure.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LockError {
    /// A file could not be read.
    Io {
        /// Path being read.
        path: String,
        /// Operating-system error detail.
        message: String,
    },
    /// Lock TOML is malformed.
    MalformedToml {
        /// Machine-readable problem detail.
        problem: String,
    },
    /// A parsed document failed contract validation.
    Invalid {
        /// Machine-readable problem detail.
        problem: String,
    },
    /// Lock and manifest disagree, or the policy mirror differs.
    Mismatch {
        /// Machine-readable problem detail.
        problem: String,
    },
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

/// Read one UTF-8 bootstrap file (lock, manifest, or version policy).
/// # Errors
pub fn load_text(path: &Path) -> Result<String, LockError> {
    std::fs::read_to_string(path).map_err(|err| LockError::Io {
        path: path.display().to_string(),
        message: err.to_string(),
    })
}

/// Parse and validate `.velnor/generator.lock` TOML text.
/// # Errors
pub fn parse_generator_lock(text: &str) -> Result<GeneratorLock, LockError> {
    let doc = parse_toml(text)?;
    let lock = lock_from_doc(&doc)?;
    lock.validate("generator.lock")
        .map_err(|err| invalid(err.to_string()))?;
    lock.check_supported_targets("generator.lock")
        .map_err(|err| invalid(err.to_string()))?;
    Ok(lock)
}

/// Parse and validate release-manifest JSON text.
/// # Errors
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

/// Verify the version-policy mirror equals the compiled catalog exactly.
///
/// Every `[tools]` pin must match [`ToolCatalog::pinned`] (plus the Mise
/// runner release); any drift fails dogfood CI per bootstrap contract §1.
/// # Errors
pub fn verify_version_policy(text: &str, catalog: &ToolCatalog) -> Result<(), LockError> {
    let doc = parse_toml(text)?;
    check_schema(&doc, 1)?;
    let tools = section(&doc, "tools")?;
    for tool in PinnedTool::ALL {
        let pinned = catalog.version(tool);
        match tools.get(tool.tool_name()) {
            Some(found) if found == pinned => {}
            Some(found) => {
                return Err(mismatch(format!(
                    "tool:{}:{found}:{pinned}",
                    tool.tool_name()
                )));
            }
            None => return Err(mismatch(format!("tool_missing:{}", tool.tool_name()))),
        }
    }
    match tools.get("mise") {
        Some(found) if found == MISE_VERSION => Ok(()),
        Some(found) => Err(mismatch(format!("tool:mise:{found}:{MISE_VERSION}"))),
        None => Err(mismatch("tool_missing:mise".to_owned())),
    }
}

/// Parse the version-policy header into its contract document.
///
/// Reads the top-level header plus the `[runner]` inventory (or the dotted
/// `[github_runner_images.linux_x64]` form); the `[tools]` mirror stays
/// [`verify_version_policy`]'s job.
/// # Errors
pub fn parse_version_policy(text: &str) -> Result<VersionPolicy, LockError> {
    let doc = parse_toml(text)?;
    check_schema(&doc, VersionPolicy::SCHEMA)?;
    let top = section(&doc, "")?;
    let inventory =
        section(&doc, "runner").or_else(|_| section(&doc, "github_runner_images.linux_x64"))?;
    Ok(VersionPolicy {
        schema: VersionPolicy::SCHEMA,
        channel: entry(&top, "channel")?,
        registry: entry(&top, "registry")?,
        check_interval_hours: entry_int(&top, "check_interval_hours")?,
        max_exception_days: entry_int(&top, "max_exception_days")?,
        github_runner_images: GithubRunnerImages {
            linux_x64: RunnerInventory {
                default: entry(&inventory, "default")?,
                supported: parse_string_list(&entry(&inventory, "supported")?)?,
            },
        },
    })
}

/// Verify the version-policy header against the contract schema.
///
/// Fails closed on malformed headers and weakened cadence. The `[tools]`
/// mirror check stays separate until the repo file carries `registry` and
/// `supported` (VER-2.19 human seed).
/// # Errors
pub fn verify_policy_header(text: &str) -> Result<(), LockError> {
    parse_version_policy(text)?
        .validate("version-policy.toml")
        .map_err(|err| invalid(err.to_string()))
}

/// Build a mismatch error.
pub(crate) fn mismatch(problem: String) -> LockError {
    LockError::Mismatch { problem }
}

/// Build an invalid-document error.
fn invalid(problem: String) -> LockError {
    LockError::Invalid { problem }
}

/// Fetch one required bare-integer entry.
fn entry_int(entries: &BTreeMap<String, String>, key: &str) -> Result<u32, LockError> {
    let value = entries.get(key).and_then(|entry| entry.parse::<u32>().ok());
    value.ok_or_else(|| invalid(format!("missing_key:{key}")))
}

/// Parse a raw `["a", "b"]` value stored opaquely by [`parse_value`].
fn parse_string_list(raw: &str) -> Result<Vec<String>, LockError> {
    let inner = raw
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .ok_or_else(|| malformed(format!("bad_list:{raw}")))?;
    let mut items = Vec::new();
    for item in inner.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        let unquoted = item
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
            .ok_or_else(|| malformed(format!("bad_list:{raw}")))?;
        items.push(unescape(unquoted)?);
    }
    Ok(items)
}

/// One parsed TOML section: dotted name plus string-or-int entries.
type TomlDoc = Vec<(String, BTreeMap<String, String>)>;

/// Parse the bootstrap TOML subset (strings, bare ints, tables, arrays).
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

/// Strip a trailing `#` comment outside double quotes.
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

/// Parse one scalar: double-quoted string, bare integer, or raw `[...]`.
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

/// Unescape `\"` and `\\` inside a double-quoted scalar.
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

/// Build a malformed-TOML error.
fn malformed(problem: String) -> LockError {
    LockError::MalformedToml { problem }
}

/// Require the top-level `schema` entry to equal `want`.
fn check_schema(doc: &TomlDoc, want: u32) -> Result<(), LockError> {
    section(doc, "")?
        .get("schema")
        .is_some_and(|schema| schema == &want.to_string())
        .then_some(())
        .ok_or_else(|| LockError::Invalid {
            problem: "bad_schema".to_owned(),
        })
}

/// Fetch one required string entry.
fn entry(entries: &BTreeMap<String, String>, key: &str) -> Result<String, LockError> {
    entries
        .get(key)
        .cloned()
        .ok_or_else(|| invalid(format!("missing_key:{key}")))
}

/// Fetch the single section with `name`.
fn section(doc: &TomlDoc, name: &str) -> Result<BTreeMap<String, String>, LockError> {
    doc.iter()
        .find(|(title, _)| title == name)
        .map(|(_, entries)| entries.clone())
        .ok_or_else(|| invalid(format!("missing_section:{name}")))
}

/// Map a parsed document onto the typed [`GeneratorLock`].
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
    })
}
