// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::MiseError;
use velnor_actions_contract::StepRole;

pub use velnor_actions_contract::workflow::cache_family::SOURCE_SUBSET;

pub const SOURCE_OBJECT_KINDS: [&str; 4] = ["source", "tool", "private_asset", "verification"];

pub const DIGEST_PREFIX: &str = "s256-";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImmutableSourceObject {
    pub object_id: String,
    pub kind: String,
    pub approved_origin_id: String,
    pub content_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceHydration {
    Warm,
    ColdFetch { missing_object_ids: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteCounter {
    pub value: Option<u64>,
    pub unknown_reason: Option<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceByteLedger {
    pub origin_bytes: ByteCounter,
    pub cache_bytes: ByteCounter,
}

pub const TRUSTED_WRITER_ROLE: &str = "plan";

pub const NEVER_ARCHIVE_MARKERS: [&str; 3] = ["credentials", ".tfstate", ".tfplan"];

#[must_use]
pub fn is_never_archive_path(path: &str) -> bool {
    NEVER_ARCHIVE_MARKERS
        .iter()
        .any(|marker| path.contains(marker))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchDecision {
    OfflineSkip,
    ExplicitFetch { miss_reason: &'static str },
}

pub fn sources_cache_paths(cargo_home: &str) -> Result<Vec<String>, MiseError> {
    if cargo_home.trim().is_empty() {
        return Err(MiseError::InvalidStepInput {
            field: "cargo_home".to_owned(),
            value: cargo_home.to_owned(),
        });
    }
    Ok(SOURCE_SUBSET
        .iter()
        .map(|suffix| format!("{cargo_home}/{suffix}"))
        .collect())
}

pub fn validate_sources_subset(paths: &[String], cargo_home: &str) -> Result<(), MiseError> {
    for path in paths {
        if path.contains("..") || is_never_archive_path(path) {
            return Err(reject(path));
        }
        if !path.starts_with(&format!("{cargo_home}/")) {
            return Err(reject(path));
        }
        let suffix = path.strip_prefix(&format!("{cargo_home}/")).unwrap_or("");
        if suffix.starts_with("registry/src") {
            return Err(reject(path));
        }
        let allowed = SOURCE_SUBSET
            .iter()
            .any(|ok| suffix == *ok || suffix.starts_with(&format!("{ok}/")));
        if !allowed {
            return Err(reject(path));
        }
    }
    Ok(())
}

fn reject(path: &str) -> MiseError {
    MiseError::InvalidStepInput {
        field: "sources_path".to_owned(),
        value: path.to_owned(),
    }
}

fn reject_object(field: &'static str, value: &str) -> MiseError {
    MiseError::InvalidStepInput {
        field: field.to_owned(),
        value: value.to_owned(),
    }
}

#[must_use]
pub fn sha256_body_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(char::from(b"0123456789abcdef"[usize::from(byte >> 4)]));
        out.push(char::from(b"0123456789abcdef"[usize::from(byte & 0x0f)]));
    }
    out
}

pub fn digest_body_hex(digest: &str) -> Result<&str, MiseError> {
    let Some(body) = digest.strip_prefix(DIGEST_PREFIX) else {
        return Err(MiseError::InvalidDigest {
            value: digest.to_owned(),
            problem: "missing_s256_prefix".to_owned(),
        });
    };
    if body.len() != 64
        || !body
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(MiseError::InvalidDigest {
            value: digest.to_owned(),
            problem: "malformed_s256_body".to_owned(),
        });
    }
    Ok(body)
}

impl ImmutableSourceObject {
    pub fn validate(&self) -> Result<(), MiseError> {
        if self.object_id.is_empty()
            || self.object_id.contains('\0')
            || self.object_id.contains("..")
        {
            return Err(reject_object("object_id", &self.object_id));
        }
        if !SOURCE_OBJECT_KINDS.contains(&self.kind.as_str()) {
            return Err(reject_object("object_kind", &self.kind));
        }
        if self.approved_origin_id.is_empty() {
            return Err(reject_object(
                "approved_origin_id",
                &self.approved_origin_id,
            ));
        }
        digest_body_hex(&self.content_digest)?;
        Ok(())
    }
}

#[must_use]
pub fn is_approved_origin(origin: &str, approved: &[&str]) -> bool {
    !origin.is_empty() && approved.contains(&origin)
}

pub fn require_approved_origins(
    required: &[ImmutableSourceObject],
    approved: &[&str],
) -> Result<(), MiseError> {
    for object in required {
        object.validate()?;
        if !is_approved_origin(&object.approved_origin_id, approved) {
            return Err(reject_object(
                "approved_origin_id",
                &object.approved_origin_id,
            ));
        }
    }
    Ok(())
}

pub fn verify_restored_object(
    object: &ImmutableSourceObject,
    restored: bool,
    bytes: &[u8],
) -> Result<bool, MiseError> {
    object.validate()?;
    if !restored {
        return Ok(false);
    }
    Ok(sha256_body_hex(bytes) == digest_body_hex(&object.content_digest)?)
}

pub fn decide_hydration(
    required: &[ImmutableSourceObject],
    verified_present_ids: &[String],
) -> Result<SourceHydration, MiseError> {
    let mut seen: Vec<&str> = Vec::with_capacity(required.len());
    for object in required {
        object.validate()?;
        if seen.contains(&object.object_id.as_str()) {
            return Err(reject_object("object_id", &object.object_id));
        }
        seen.push(&object.object_id);
    }
    let mut missing: Vec<String> = Vec::new();
    for object in required {
        if !verified_present_ids
            .iter()
            .any(|present| present == &object.object_id)
            && !missing.iter().any(|known| known == &object.object_id)
        {
            missing.push(object.object_id.clone());
        }
    }
    if missing.is_empty() {
        Ok(SourceHydration::Warm)
    } else {
        Ok(SourceHydration::ColdFetch {
            missing_object_ids: missing,
        })
    }
}

impl ByteCounter {
    #[must_use]
    pub const fn measured(value: u64) -> Self {
        Self {
            value: Some(value),
            unknown_reason: None,
        }
    }

    #[must_use]
    pub const fn unknown(reason: &'static str) -> Self {
        Self {
            value: None,
            unknown_reason: Some(reason),
        }
    }

    pub fn validate(&self) -> Result<(), MiseError> {
        match (&self.value, &self.unknown_reason) {
            (Some(_), None) | (None, Some(_)) => Ok(()),
            _ => Err(MiseError::InvalidStepInput {
                field: "byte_counter".to_owned(),
                value: "exactly_one_of_value_or_unknown_reason".to_owned(),
            }),
        }
    }

    pub fn record(&mut self, bytes: u64) -> Result<(), MiseError> {
        match self.value {
            Some(current) => {
                let next = current
                    .checked_add(bytes)
                    .ok_or(MiseError::InvalidStepInput {
                        field: "byte_counter".to_owned(),
                        value: "byte_overflow".to_owned(),
                    })?;
                self.value = Some(next);
                Ok(())
            }
            None => Err(MiseError::InvalidStepInput {
                field: "byte_counter".to_owned(),
                value: "bytes_unknown".to_owned(),
            }),
        }
    }
}

impl SourceByteLedger {
    #[must_use]
    pub const fn new(origin_bytes: ByteCounter, cache_bytes: ByteCounter) -> Self {
        Self {
            origin_bytes,
            cache_bytes,
        }
    }

    pub fn validate(&self) -> Result<(), MiseError> {
        self.origin_bytes.validate()?;
        self.cache_bytes.validate()?;
        Ok(())
    }

    pub fn record_origin(&mut self, bytes: u64) -> Result<(), MiseError> {
        self.origin_bytes.record(bytes)
    }

    pub fn record_cache(&mut self, bytes: u64) -> Result<(), MiseError> {
        self.cache_bytes.record(bytes)
    }
}

#[must_use]
pub fn is_trusted_writer(role: &str) -> bool {
    role == TRUSTED_WRITER_ROLE
}

pub fn fetch_decision(
    sources_complete: bool,
    miss_reason: &'static str,
) -> Result<FetchDecision, MiseError> {
    if sources_complete {
        return Ok(FetchDecision::OfflineSkip);
    }
    if !matches!(
        miss_reason,
        "no_entry" | "source_missing" | "cache_unavailable" | "cache_corrupt"
    ) {
        return Err(MiseError::InvalidStepInput {
            field: "miss_reason".to_owned(),
            value: miss_reason.to_owned(),
        });
    }
    Ok(FetchDecision::ExplicitFetch { miss_reason })
}

pub fn check_restore_before_fetch(
    roles: &[Option<StepRole>],
    has_mbx: bool,
) -> Result<(), MiseError> {
    let fetches = roles
        .iter()
        .enumerate()
        .filter_map(|(index, role)| (*role == Some(StepRole::CargoSourcesFetch)).then_some(index));
    for fetch_at in fetches {
        let source_restore = roles
            .iter()
            .position(|role| *role == Some(StepRole::CargoSourcesRestore));
        if source_restore.is_some_and(|restore_at| fetch_at < restore_at) {
            return Err(MiseError::CacheNotEligible {
                task: "fetch".to_owned(),
                reason: "fetch_before_restore".to_owned(),
            });
        }
        if has_mbx {
            let mbx_restore = roles
                .iter()
                .position(|role| *role == Some(StepRole::MbxCache));
            if mbx_restore.is_some_and(|restore_at| fetch_at < restore_at) {
                return Err(MiseError::CacheNotEligible {
                    task: "fetch".to_owned(),
                    reason: "fetch_before_mbx".to_owned(),
                });
            }
        }
    }
    Ok(())
}

pub fn check_steps_before_fetch(names: &[String], required: &[&str]) -> Result<(), MiseError> {
    let at = |want: &str| names.iter().position(|name| name == want);
    let fetch = at("Fetch Cargo sources").or_else(|| {
        names
            .iter()
            .position(|n| n.starts_with("Fetch Cargo sources"))
    });
    if let Some(fetch_at) = fetch {
        for required_name in required {
            if let Some(required_at) = names.iter().position(|name| name == required_name)
                && fetch_at < required_at
            {
                return Err(MiseError::CacheNotEligible {
                    task: "fetch".to_owned(),
                    reason: "fetch_before_required_step".to_owned(),
                });
            }
        }
    }
    Ok(())
}
