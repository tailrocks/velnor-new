// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;

use sha2::{Digest, Sha256};

use crate::errors::ContractError;

use super::CacheFamily;
use super::state_unit::{SLICE_ID_MAX_BYTES, SliceId, StateUnit};

pub const MBX_COMPILE_ENV: [(&str, &str); 3] = [
    ("MBX_SHARE_OUT_DIR", "1"),
    ("MBX_CC", "0"),
    ("SOURCE_DATE_EPOCH", "0"),
];

pub const MBX_BSE_ENV: &str = "MBX_BUILD_SCRIPT_EXECUTION";

pub const MBX_BSE_PRODUCER: &str = "0";

pub const MBX_BSE_SKELETON: &str = "1";

pub const MBX_BSE_CONSUMER: &str = "1";

pub const MBX_BSE_COMPAT: &str = "hybrid";

pub const ROOT_SLUG_MAX_BYTES: usize = 48;

const DIGEST_PREFIX: &str = "s256-";
const DIGEST_HEX_BYTES: usize = 64;
const ROOT_SLUG_FOR_DOT: &str = "root";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct S256([u8; 32]);

impl S256 {
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    pub fn parse(text: &str) -> Result<Self, ContractError> {
        let hex = text
            .strip_prefix(DIGEST_PREFIX)
            .filter(|hex| {
                hex.len() == DIGEST_HEX_BYTES
                    && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
            })
            .ok_or_else(|| ContractError::identity("state.digest", "not_s256"))?;
        let mut bytes = [0_u8; 32];
        for (slot, pair) in bytes.iter_mut().zip(hex.as_bytes().chunks(2)) {
            let nibble = |b: u8| if b <= b'9' { b - b'0' } else { b - b'a' + 10 };
            *slot = nibble(pair[0]) << 4 | nibble(pair[1]);
        }
        Ok(Self(bytes))
    }
}

impl fmt::Display for S256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(DIGEST_PREFIX)?;
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02x}"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RootSlug(String);

impl RootSlug {
    pub fn from_root(root: &str) -> Result<Self, ContractError> {
        let slug: String = match root.trim_end_matches('/') {
            "" | "." => ROOT_SLUG_FOR_DOT.to_owned(),
            dir => dir
                .chars()
                .map(|c| match c {
                    '/' | '.' | '_' => '-',
                    other => other.to_ascii_lowercase(),
                })
                .collect(),
        };
        let shaped = !slug.is_empty()
            && slug.len() <= ROOT_SLUG_MAX_BYTES
            && slug.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
            && slug
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !shaped {
            return Err(ContractError::identity(
                "state.root",
                format!("root_not_sluggable:{root}"),
            ));
        }
        if format!("-{slug}-").contains("-s256-") {
            return Err(ContractError::identity(
                "state.root",
                format!("root_collides_with_digest_delimiter:{root}"),
            ));
        }
        Ok(Self(slug))
    }

    pub fn unique(roots: &[&str]) -> Result<Vec<Self>, ContractError> {
        let mut slugs: Vec<Self> = Vec::with_capacity(roots.len());
        for root in roots {
            let slug = Self::from_root(root)?;
            if slugs.contains(&slug) {
                return Err(ContractError::identity(
                    "state.root",
                    format!("root_slug_collision:{root}:{slug}"),
                ));
            }
            slugs.push(slug);
        }
        Ok(slugs)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RootSlug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateTrust {
    Trusted,
    Pr,
}

impl StateTrust {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Trusted => "trusted",
            Self::Pr => "pr",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateScope {
    pub trust: StateTrust,
    pub unit: StateUnit,
}

impl StateScope {
    #[must_use]
    pub fn prefix(&self) -> String {
        format!(
            "{}{}-{}-",
            CacheFamily::MbxBundle.key_prefix(),
            self.trust.as_str(),
            self.unit
        )
    }

    #[must_use]
    pub fn restore_prefix(&self, compat: &S256) -> String {
        format!("{}{compat}-", self.prefix())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateCompat {
    pub unit: String,
    pub root: String,
    pub mbx: String,
    pub toolchain: String,
    pub image_os: String,
    pub uname_s: String,
    pub uname_m: String,
    pub store: String,
    pub checkout: String,
}

impl StateCompat {
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        let spec = CacheFamily::MbxBundle.spec();
        let mut lines = vec![
            format!("family={}", spec.layer),
            format!("schema={}", spec.schema),
        ];
        lines.extend(spec.shell_paths(None));
        lines.extend([
            format!("unit={}", self.unit),
            format!("mbx={}", self.mbx),
            format!("toolchain={}", self.toolchain),
            format!("root={}", self.root),
        ]);
        lines.extend(
            MBX_COMPILE_ENV
                .iter()
                .map(|(name, value)| format!("{name}={value}")),
        );
        lines.push(format!("{MBX_BSE_ENV}={MBX_BSE_COMPAT}"));
        lines.extend([
            format!("uname_s={}", self.uname_s),
            format!("uname_m={}", self.uname_m),
            format!("ImageOS={}", self.image_os),
            format!("store={}", self.store),
            format!("checkout={}", self.checkout),
        ]);
        lines
    }

    #[must_use]
    pub fn digest(&self) -> S256 {
        digest_of_lines(&self.lines())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateSnapshot {
    pub lock: S256,
    pub rust_inputs: S256,
    pub task_set: S256,
    pub producer_recipe: S256,
}

impl StateSnapshot {
    pub const LINE_NAMES: [&'static str; 4] =
        ["lock", "rust_inputs", "task_set", "producer_recipe"];

    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        Self::LINE_NAMES
            .iter()
            .zip([
                self.lock,
                self.rust_inputs,
                self.task_set,
                self.producer_recipe,
            ])
            .map(|(name, digest)| format!("{name}={digest}"))
            .collect()
    }

    #[must_use]
    pub fn digest(&self) -> S256 {
        digest_of_lines(&self.lines())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateIdentity {
    pub scope: StateScope,
    pub compat: S256,
    pub snapshot: S256,
}

impl StateIdentity {
    #[must_use]
    pub fn snapshot_prefix(&self) -> String {
        format!(
            "{}{}-",
            self.scope.restore_prefix(&self.compat),
            self.snapshot
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateEntryKey {
    pub identity: StateIdentity,
    pub payload: S256,
}

impl StateEntryKey {
    #[must_use]
    pub fn render(&self) -> String {
        format!("{}{}", self.identity.snapshot_prefix(), self.payload)
    }
}

fn digest_of_lines(lines: &[String]) -> S256 {
    let mut text = lines.join("\n");
    text.push('\n');
    S256::of(text.as_bytes())
}

pub(super) fn widest_entry_key_bytes() -> usize {
    let zero = S256::of(b"");
    let widest = "a".repeat(SLICE_ID_MAX_BYTES.max(ROOT_SLUG_MAX_BYTES));
    let key = StateEntryKey {
        identity: StateIdentity {
            scope: StateScope {
                trust: StateTrust::Trusted,
                unit: StateUnit::Slice(SliceId(widest)),
            },
            compat: zero,
            snapshot: zero,
        },
        payload: zero,
    };
    key.render().len()
}
