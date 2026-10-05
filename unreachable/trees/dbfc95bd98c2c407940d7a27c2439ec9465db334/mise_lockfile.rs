//! Read-only `mise.lock` hygiene audit (G2).
//!
//! The audit verifies the committed lock is complete and well-formed
//! for local `mise install`: every emitted install spec must resolve
//! to a lock entry, and a verifiable entry with a CI-platform hole
//! (or a corrupt checksum) blocks generation instead of shipping a
//! lock that cannot install the pinned set.
//!
//! Hygiene only, not runtime verification: CI installs run
//! `--no-config` isolated and trust upstream TLS plus exact pinned
//! versions. Lock checksums are TOFU (trust on first use).
//!
//! Provenance boundary: the committed lock is trusted input like
//! `mise.toml` itself. The audit checks coverage and shape only — it
//! cannot recompute upstream bytes. A self-consistent malicious lock
//! (attacker URL plus matching checksum) is a malicious commit, out
//! of scope.
//!
//! Backend boundary: core backends (notably `rust`) emit no platform
//! checksums at all (`mise lock`: "0 platform entries, 7 skipped"),
//! so checksum-less entries are advisory, never blocking. Blocking
//! fires only when the entry proves checksums exist (other platforms
//! present) but omits the CI platform, or when a CI checksum is
//! corrupt (mise would fail that local install unconditionally).

use std::collections::BTreeMap;

use crate::catalog::{PinnedTool, ToolCatalog};

/// One locked tool: pinned version plus per-platform checksums.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockEntry {
    /// Locked version string.
    pub version: String,
    /// Mise platform (`linux-x64`) to raw `checksum` value.
    pub checksums: BTreeMap<String, String>,
}

/// Parsed `mise.lock`: config tool key to locked entry.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MiseLockfile {
    /// Locked entries keyed by config tool key.
    pub tools: BTreeMap<String, LockEntry>,
}

/// Verification coverage of one installed tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallCoverage {
    /// Locked version matches the pin with a well-formed CI checksum.
    Verified,
    /// Locked version differs from the pin (stale lock, advisory).
    VersionDrift {
        /// Version the lock pins.
        locked: String,
    },
    /// No lock entry under any key for this tool (advisory).
    MissingEntry,
    /// Pin-matching entry carries no checksums at all (core backends
    /// like `rust` emit none; legacy formats neither — advisory).
    NoChecksums,
    /// Pin-matching entry checksums other platforms but not the CI
    /// platform (blocking: regenerate with `mise lock`).
    MissingPlatform {
        /// Platforms the entry does cover, sorted.
        locked_platforms: Vec<String>,
    },
    /// Pin-matching CI checksum is malformed (blocking: mise fails
    /// the install unconditionally, so generation must fail first).
    CorruptChecksum {
        /// Observed checksum value.
        observed: String,
    },
}

impl InstallCoverage {
    /// True for the fail-closed variants (platform hole, corrupt shape).
    #[must_use]
    pub fn is_blocking(&self) -> bool {
        matches!(
            self,
            Self::MissingPlatform { .. } | Self::CorruptChecksum { .. }
        )
    }
}

/// Mise platform for a Rust target triple, when mise names it.
#[must_use]
pub fn mise_platform_for_target(triple: &str) -> Option<&'static str> {
    match triple {
        "x86_64-unknown-linux-gnu" => Some("linux-x64"),
        "aarch64-apple-darwin" => Some("macos-arm64"),
        "x86_64-apple-darwin" => Some("macos-x64"),
        _ => None,
    }
}

/// Parse `mise.lock` v3 bytes into locked entries.
///
/// Strict lines, lenient tables: structural garbage fails the whole
/// parse (advisory — mise itself warns and proceeds unverified), while
/// unknown tables and keys are ignored for forward compatibility.
/// # Errors
pub fn parse_mise_lockfile(text: &str) -> Result<MiseLockfile, String> {
    let mut lock = MiseLockfile::default();
    let mut tool: Option<String> = None;
    let mut platform: Option<String> = None;
    for (index, line) in text.lines().enumerate() {
        let code = line.trim();
        if code.is_empty() || code.starts_with('#') {
            continue;
        }
        if let Some(header) = parse_header(code, index)? {
            match header {
                Header::Tool(key) => {
                    if lock.tools.contains_key(&key) {
                        return Err(line_problem(index, "duplicate_tool"));
                    }
                    lock.tools.insert(
                        key.clone(),
                        LockEntry {
                            version: String::new(),
                            checksums: BTreeMap::new(),
                        },
                    );
                    tool = Some(key);
                    platform = None;
                }
                Header::Platform(key, name) => {
                    if tool.as_ref() != Some(&key) || !lock.tools.contains_key(&key) {
                        return Err(line_problem(index, "platform_without_tool"));
                    }
                    platform = Some(name);
                }
                Header::Other => {
                    tool = None;
                    platform = None;
                }
            }
            continue;
        }
        let (key, value) = split_setting(code, index)?;
        match (tool.as_ref(), platform.as_ref(), key) {
            (Some(name), None, "version") => {
                let entry = lock
                    .tools
                    .get_mut(name)
                    .ok_or_else(|| line_problem(index, "version_without_tool"))?;
                if !entry.version.is_empty() {
                    return Err(line_problem(index, "duplicate_key"));
                }
                entry.version = value;
            }
            (Some(name), Some(platform), "checksum") => {
                let entry = lock
                    .tools
                    .get_mut(name)
                    .ok_or_else(|| line_problem(index, "checksum_without_tool"))?;
                if entry.checksums.contains_key(platform) {
                    return Err(line_problem(index, "duplicate_key"));
                }
                entry.checksums.insert(platform.clone(), value);
            }
            _ => {}
        }
    }
    Ok(lock)
}

/// One parsed section header.
enum Header {
    /// `[[tools.<key>]]` entry.
    Tool(String),
    /// `[tools.<key>."platforms.<platform>"]` checksums.
    Platform(String, String),
    /// Anything else (ignored for forward compatibility).
    Other,
}

/// Parse a `[`/`[[` header line.
fn parse_header(code: &str, index: usize) -> Result<Option<Header>, String> {
    if !code.starts_with('[') {
        return Ok(None);
    }
    if let Some(key) = code
        .strip_prefix("[[tools.")
        .and_then(|rest| rest.strip_suffix("]]"))
    {
        return Ok(Some(Header::Tool(unquote(key.trim(), index)?)));
    }
    let Some(inner) = code
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
    else {
        return Err(line_problem(index, "unterminated_section"));
    };
    let Some(rest) = inner.strip_prefix("tools.") else {
        return Ok(Some(Header::Other));
    };
    let Some((key, platform)) = rest.rsplit_once(r#"."platforms."#) else {
        return Ok(Some(Header::Other));
    };
    let platform = platform
        .strip_suffix('"')
        .ok_or_else(|| line_problem(index, "unterminated_platform"))?;
    if key.is_empty() || platform.is_empty() {
        return Err(line_problem(index, "empty_platform_key"));
    }
    Ok(Some(Header::Platform(
        unquote(key.trim(), index)?,
        platform.to_owned(),
    )))
}

/// Strip one pair of double quotes, if present.
fn unquote(key: &str, index: usize) -> Result<String, String> {
    if !key.starts_with('"') {
        if key.is_empty() {
            return Err(line_problem(index, "empty_tool_key"));
        }
        return Ok(key.to_owned());
    }
    if key.len() >= 2 && key.ends_with('"') {
        return Ok(key[1..key.len() - 1].to_owned());
    }
    Err(line_problem(index, "unterminated_key"))
}

/// Split one `key = "value"` (or `key = 123`) setting.
fn split_setting(code: &str, index: usize) -> Result<(&str, String), String> {
    let (key, value) = code
        .split_once('=')
        .ok_or_else(|| line_problem(index, "expected_key_equals_value"))?;
    let key = key.trim();
    let trimmed = value.trim();
    let value = trimmed
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .map(str::to_owned)
        .or_else(|| {
            let integer = !trimmed.is_empty() && trimmed.bytes().all(|b| b.is_ascii_digit());
            let array = trimmed.starts_with('[') && trimmed.ends_with(']');
            (integer || array).then(|| trimmed.to_owned())
        });
    match (key.is_empty(), value) {
        (false, Some(value)) => Ok((key, value)),
        _ => Err(line_problem(index, "expected_quoted_string")),
    }
}

/// `line N: problem` detail with one-based line numbers.
fn line_problem(index: usize, problem: &str) -> String {
    format!("line {}: {problem}", index.saturating_add(1))
}

/// Lock keys a catalog tool may appear under: the registry name plus
/// the backend-qualified spec prefix (Nextest's aqua path).
fn lock_keys_for(tool: PinnedTool, catalog: &ToolCatalog) -> Vec<String> {
    let mut keys = vec![tool.tool_name().to_owned()];
    let spec = catalog.tool_spec(tool);
    if let Some(prefix) = spec.split_once('@').map(|(head, _)| head.to_owned())
        && prefix != keys[0]
    {
        keys.push(prefix);
    }
    keys
}

/// One emitted install to audit: display name, expected pin version,
/// and the exact lock key the emitted spec addresses.
///
/// The lock key is the spec's key verbatim, never a first match over
/// aliases: mise honors the entry under the key the install names, so
/// an entry under any other key is invisible to that install (a decoy
/// entry must never mask a hole or a corrupt checksum).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallSubject {
    /// `tool@pin` display name for findings.
    pub display: String,
    /// Pin version the lock entry must carry.
    pub expected_version: String,
    /// Lock key the emitted spec addresses.
    pub lock_key: String,
}

/// Resolve an emitted install spec (`key@version`) to its audit subject.
///
/// The key must name a catalog tool (registry name or backend-qualified
/// spec prefix); the recorded lock key is the spec's key verbatim. The
/// spec version must equal the catalog pin exactly: a drifted spec is
/// unauditable (the caller blocks it), never silently re-pinned to the
/// pin it should have carried. Lock-side drift stays a later advisory.
#[must_use]
pub fn subject_for_install_spec(spec: &str, catalog: &ToolCatalog) -> Option<InstallSubject> {
    let (key, version) = spec.split_once('@')?;
    if key.is_empty() || version.is_empty() {
        return None;
    }
    let tool = PinnedTool::ALL.iter().find(|tool| {
        lock_keys_for(**tool, catalog)
            .iter()
            .any(|known| known == key)
    })?;
    if version != catalog.version(*tool) {
        return None;
    }
    Some(InstallSubject {
        display: format!("{}@{}", tool.tool_name(), catalog.version(*tool)),
        expected_version: catalog.version(*tool).to_owned(),
        lock_key: key.to_owned(),
    })
}

/// True for a checksum shape mise can enforce (`scheme:hex`).
fn is_enforceable_checksum(value: &str) -> bool {
    value
        .split_once(':')
        .is_some_and(|(scheme, hex)| !scheme.is_empty() && !hex.is_empty() && is_hex(hex))
}

/// True for nonempty hex in either case (mise accepts uppercase).
fn is_hex(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Audit emitted installs against the parsed lock for one platform.
///
/// Subjects iterate in caller order; verdicts are per subject.
/// Shape-only: a well-formed but wrong checksum still fails closed at
/// local install time through mise's native enforcement.
#[must_use]
pub fn audit_install_coverage(
    lock: &MiseLockfile,
    subjects: &[InstallSubject],
    platform: &str,
) -> Vec<InstallCoverage> {
    subjects
        .iter()
        .map(|subject| coverage_for_subject(lock, subject, platform))
        .collect()
}

/// Coverage of one install: entry match, version match, CI checksum shape.
///
/// Looks up the subject's lock key only: any other key's entry is
/// invisible to this install and must not affect its verdict.
fn coverage_for_subject(
    lock: &MiseLockfile,
    subject: &InstallSubject,
    platform: &str,
) -> InstallCoverage {
    let Some(entry) = lock.tools.get(&subject.lock_key) else {
        return InstallCoverage::MissingEntry;
    };
    if entry.version != subject.expected_version {
        return InstallCoverage::VersionDrift {
            locked: entry.version.clone(),
        };
    }
    match entry.checksums.get(platform) {
        Some(checksum) if is_enforceable_checksum(checksum) => InstallCoverage::Verified,
        Some(checksum) => InstallCoverage::CorruptChecksum {
            observed: checksum.clone(),
        },
        None if entry.checksums.is_empty() => InstallCoverage::NoChecksums,
        None => InstallCoverage::MissingPlatform {
            locked_platforms: entry.checksums.keys().cloned().collect(),
        },
    }
}
