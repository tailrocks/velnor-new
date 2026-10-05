//! Strict semver versions plus Cargo-style requirement matching.
//!
//! Dependency-free: only the subset Cargo accepts for member versions and
//! dependency requirements is implemented. Build metadata is validated and
//! then ignored for precedence, per the semver specification.

use std::cmp::Ordering;

/// One prerelease identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreIdent {
    /// Numeric identifier (no leading zeros).
    Numeric(u64),
    /// Alphanumeric identifier.
    Alpha(String),
}

/// One `major.minor.patch[-pre][+build]` version ordered by precedence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemVersion {
    /// Major component.
    pub major: u64,
    /// Minor component.
    pub minor: u64,
    /// Patch component.
    pub patch: u64,
    /// Prerelease identifiers (empty for a stable release).
    pub pre: Vec<PreIdent>,
}

impl PartialOrd for SemVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SemVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        core_parts(self)
            .cmp(&core_parts(other))
            .then_with(|| cmp_pre(&self.pre, &other.pre))
    }
}

/// Core components for ordering.
fn core_parts(version: &SemVersion) -> (u64, u64, u64) {
    (version.major, version.minor, version.patch)
}

/// Compare prerelease identifier lists (absent release outranks prerelease).
fn cmp_pre(left: &[PreIdent], right: &[PreIdent]) -> Ordering {
    match (left.is_empty(), right.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => cmp_pre_lists(left, right),
    }
}

/// Compare two non-empty prerelease identifier lists.
fn cmp_pre_lists(left: &[PreIdent], right: &[PreIdent]) -> Ordering {
    for pair in left.iter().zip(right.iter()) {
        let order = cmp_pre_ident(pair.0, pair.1);
        if order != Ordering::Equal {
            return order;
        }
    }
    left.len().cmp(&right.len())
}

/// Compare two prerelease identifiers (numeric sorts below alphanumeric).
fn cmp_pre_ident(left: &PreIdent, right: &PreIdent) -> Ordering {
    match (left, right) {
        (PreIdent::Numeric(left), PreIdent::Numeric(right)) => left.cmp(right),
        (PreIdent::Alpha(left), PreIdent::Alpha(right)) => left.cmp(right),
        (PreIdent::Numeric(_), PreIdent::Alpha(_)) => Ordering::Less,
        (PreIdent::Alpha(_), PreIdent::Numeric(_)) => Ordering::Greater,
    }
}

/// Whether `text` is a numeric component without leading zeros.
fn parse_component(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if text.len() > 1 && text.starts_with('0') {
        return None;
    }
    text.parse::<u64>().ok()
}

/// Whether every character is legal inside prerelease/build identifiers.
fn ident_char_ok(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-'
}

/// Parse one prerelease section into identifiers.
fn parse_pre(text: &str) -> Option<Vec<PreIdent>> {
    let mut idents = Vec::new();
    for part in text.split('.') {
        if part.is_empty() || !part.bytes().all(ident_char_ok) {
            return None;
        }
        if part.bytes().all(|byte| byte.is_ascii_digit()) {
            let value = parse_component(part)?;
            idents.push(PreIdent::Numeric(value));
        } else {
            idents.push(PreIdent::Alpha(part.to_owned()));
        }
    }
    Some(idents)
}

/// Parse a strict semver version; build metadata is validated, then dropped.
#[must_use]
pub fn parse_version(text: &str) -> Option<SemVersion> {
    let (core, build) = match text.split_once('+') {
        Some((core, build)) => (core, Some(build)),
        None => (text, None),
    };
    if let Some(build) = build {
        let valid = !build.is_empty()
            && build
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(ident_char_ok));
        if !valid {
            return None;
        }
    }
    let (core, pre) = match core.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (core, None),
    };
    let mut parts = core.split('.');
    let major = parse_component(parts.next()?)?;
    let minor = parse_component(parts.next()?)?;
    let patch = parse_component(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    let pre = pre.map_or_else(|| Some(Vec::new()), parse_pre)?;
    Some(SemVersion {
        major,
        minor,
        patch,
        pre,
    })
}

/// Requirement operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    /// Compatible release (`^`, the default).
    Caret,
    /// Patch-level release (`~`).
    Tilde,
    /// Exact core (`=`).
    Exact,
    /// Greater than (`>`).
    Gt,
    /// Greater than or equal (`>=`).
    Gte,
    /// Less than (`<`).
    Lt,
    /// Less than or equal (`<=`).
    Lte,
}

/// One comma-separated comparator of a requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Comparator {
    /// Operator.
    op: Op,
    /// Core components (missing parts are zero).
    parts: [u64; 3],
    /// Number of specified core components.
    len: u8,
    /// Prerelease gate (empty unless the comparator names one).
    pre: Vec<PreIdent>,
    /// Whether a `*` component appears.
    wildcard: bool,
}

/// One parsed version requirement (comma-separated comparators).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionReq {
    /// Comparators (all must match).
    comparators: Vec<Comparator>,
}

/// Split one comparator into its operator and version text.
fn split_op(text: &str) -> (Op, &str) {
    for (prefix, op) in [
        (">=", Op::Gte),
        ("<=", Op::Lte),
        ("^", Op::Caret),
        ("~", Op::Tilde),
        ("=", Op::Exact),
        (">", Op::Gt),
        ("<", Op::Lt),
    ] {
        if let Some(rest) = text.strip_prefix(prefix) {
            return (op, rest.trim_start());
        }
    }
    (Op::Caret, text)
}

/// Parse the core (and optional prerelease) of one comparator.
fn parse_comparator_core(text: &str) -> Option<([u64; 3], u8, Vec<PreIdent>, bool)> {
    let (core, pre) = match text.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (text, None),
    };
    let mut parts = [0_u64; 3];
    let mut count = 0_u8;
    let mut len = 0_u8;
    let mut wildcard = false;
    for segment in core.split('.') {
        count = count.saturating_add(1);
        if count > 3 {
            return None;
        }
        if segment == "*" {
            wildcard = true;
        } else {
            if wildcard {
                return None;
            }
            parts = push_part(parts, len, parse_component(segment)?)?;
            len = len.saturating_add(1);
        }
    }
    if wildcard && pre.is_some() {
        return None;
    }
    let pre = pre.map_or_else(|| Some(Vec::new()), parse_pre)?;
    Some((parts, len, pre, wildcard))
}

/// Append one numeric component at position `len`.
fn push_part(parts: [u64; 3], len: u8, value: u64) -> Option<[u64; 3]> {
    let [first, second, third] = parts;
    match len {
        0 => Some([value, second, third]),
        1 => Some([first, value, third]),
        2 => Some([first, second, value]),
        _ => None,
    }
}

/// Parse one comma-separated comparator.
fn parse_comparator(text: &str) -> Option<Comparator> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed == "*" {
        return Some(Comparator {
            op: Op::Caret,
            parts: [0, 0, 0],
            len: 0,
            pre: Vec::new(),
            wildcard: true,
        });
    }
    let (op, rest) = split_op(trimmed);
    let (parts, len, pre, wildcard) = parse_comparator_core(rest)?;
    if wildcard && !matches!(op, Op::Caret | Op::Exact) {
        return None;
    }
    Some(Comparator {
        op,
        parts,
        len,
        pre,
        wildcard,
    })
}

/// Parse a Cargo-style version requirement.
#[must_use]
pub fn parse_req(text: &str) -> Option<VersionReq> {
    if text.trim().is_empty() {
        return None;
    }
    let mut comparators = Vec::new();
    for part in text.split(',') {
        comparators.push(parse_comparator(part)?);
    }
    Some(VersionReq { comparators })
}

/// Whether any comparator opts into `version`'s prerelease line.
fn allows_pre(req: &VersionReq, version: &SemVersion) -> bool {
    req.comparators.iter().any(|comparator| {
        let [major, minor, patch] = comparator.parts;
        !comparator.pre.is_empty()
            && major == version.major
            && minor == version.minor
            && patch == version.patch
    })
}

/// Compare `version` against comparator core plus its prerelease gate.
fn cmp_gated(version: &SemVersion, parts: &[u64; 3], pre: &[PreIdent]) -> Ordering {
    let [major, minor, patch] = *parts;
    (version.major, version.minor, version.patch)
        .cmp(&(major, minor, patch))
        .then_with(|| {
            if pre.is_empty() {
                Ordering::Equal
            } else {
                cmp_pre(&version.pre, pre)
            }
        })
}

/// Caret upper bound: increment the leftmost nonzero specified component.
fn caret_upper(parts: &[u64; 3], len: u8) -> (u64, u64, u64) {
    let [major, minor, patch] = *parts;
    if major > 0 || len <= 1 {
        return (major.saturating_add(1), 0, 0);
    }
    if minor > 0 || len == 2 {
        return (0, minor.saturating_add(1), 0);
    }
    if patch > 0 {
        (0, 0, patch.saturating_add(1))
    } else {
        (0, 0, 1)
    }
}

/// Whether one comparator accepts `version` (prerelease already gated).
fn comparator_matches(comparator: &Comparator, version: &SemVersion) -> bool {
    let [major, minor, patch] = comparator.parts;
    if comparator.wildcard {
        return match comparator.len {
            0 => true,
            1 => major == version.major,
            2 => major == version.major && minor == version.minor,
            _ => major == version.major && minor == version.minor && patch == version.patch,
        };
    }
    let order = cmp_gated(version, &comparator.parts, &comparator.pre);
    match comparator.op {
        Op::Exact => order == Ordering::Equal,
        Op::Gt => order == Ordering::Greater,
        Op::Gte => order != Ordering::Less,
        Op::Lt => order == Ordering::Less,
        Op::Lte => order != Ordering::Greater,
        Op::Caret => {
            let upper = caret_upper(&comparator.parts, comparator.len);
            order != Ordering::Less && core_parts(version) < upper
        }
        Op::Tilde => {
            let upper = tilde_upper(&comparator.parts, comparator.len);
            order != Ordering::Less && core_parts(version) < upper
        }
    }
}

/// Tilde upper bound: minor (or major for a bare major) may not advance.
fn tilde_upper(parts: &[u64; 3], len: u8) -> (u64, u64, u64) {
    let [major, minor, _] = *parts;
    if len <= 1 {
        (major.saturating_add(1), 0, 0)
    } else {
        (major, minor.saturating_add(1), 0)
    }
}

/// Whether `version` satisfies every comparator of `req`.
#[must_use]
pub fn req_matches(req: &VersionReq, version: &SemVersion) -> bool {
    if !version.pre.is_empty() && !allows_pre(req, version) {
        return false;
    }
    req.comparators
        .iter()
        .all(|comparator| comparator_matches(comparator, version))
}

/// Whether `version_text` parses and satisfies `req_text`.
#[must_use]
pub fn version_satisfies(req_text: &str, version_text: &str) -> bool {
    match (parse_req(req_text), parse_version(version_text)) {
        (Some(req), Some(version)) => req_matches(&req, &version),
        _ => false,
    }
}
