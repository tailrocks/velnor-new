// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

use super::super::identity::QualificationCacheLayer;
use super::super::runtime::QualificationRuntimeIdentity;
use crate::workflow::{QualificationPhase, QualificationRunRef};

pub const QUALIFICATION_CACHE_RECEIPT_ARTIFACT: &str = "velnor-qualification-cache-receipt-v2";

pub const QUALIFICATION_CACHE_RECEIPT_FILENAME: &str = "qualification-cache-receipt.json";

pub const MAX_QUALIFICATION_RECEIPT_BYTES: usize = 524_288;

pub const MAX_QUALIFICATION_RECEIPT_DEPTH: usize = 4;

pub(super) const MAX_JSON_NESTING: usize = 32;

pub(super) const MAX_RECEIPT_TEXT_BYTES: usize = 4_096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationCacheRestoreResult {
    Miss,

    Hit,

    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualificationCacheSaveActionResult {
    Succeeded,

    Skipped,

    NotRequired,

    Failed,

    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheBackendEntry {
    pub id: u64,

    pub key: String,

    pub git_ref: String,

    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "entry", rename_all = "snake_case")]
pub enum QualificationCacheBackendObservation {
    Absent,

    Found(QualificationCacheBackendEntry),

    NotQueried,

    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheRestore {
    pub requested_key: Option<String>,

    pub matched_key: Option<String>,

    pub matched_cache: QualificationCacheBackendObservation,

    pub result: QualificationCacheRestoreResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheSave {
    pub requested_key: Option<String>,

    pub action: QualificationCacheSaveActionResult,

    pub before: QualificationCacheBackendObservation,

    pub after: QualificationCacheBackendObservation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheLayerReceipt {
    pub layer: QualificationCacheLayer,

    pub active: bool,

    pub identity_digest: String,

    pub runtime_identity: Option<QualificationRuntimeIdentity>,

    pub state_digest: Option<String>,

    pub restore: QualificationCacheRestore,

    pub save: QualificationCacheSave,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheLaneReceipt {
    pub matrix_key: String,

    pub stack_id: String,

    pub task_id: String,

    pub completed_task_ids: Vec<String>,

    pub closure_digest: String,

    pub useful_state_digest: String,

    pub layers: Vec<QualificationCacheLayerReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheReceiptLink {
    pub run: QualificationRunRef,

    pub receipt_digest: String,

    pub artifact_id: u64,

    pub artifact_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheReceipt {
    pub schema: u32,

    pub plan_id: String,

    pub run: QualificationRunRef,

    pub campaign: String,

    pub phase: QualificationPhase,

    pub source_sha: String,

    pub configuration_digest: String,

    #[serde(default)]
    pub source_delta: Option<QualificationSourceDelta>,

    pub predecessor: Option<QualificationCacheReceiptLink>,

    pub lanes: Vec<QualificationCacheLaneReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationSourceDelta {
    pub base_source_sha: String,

    pub source_sha: String,

    pub changed_paths: Vec<String>,

    pub diff_digest: String,

    pub base_is_ancestor: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheRunMetadata {
    pub repository: String,

    pub default_branch: String,

    pub git_ref: String,

    pub ref_protected: bool,

    pub workflow_path_ref: String,

    pub workflow_ref: String,

    pub workflow_sha: String,

    pub head_sha: String,

    pub event: String,

    pub conclusion: String,

    pub run: QualificationRunRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheArtifact {
    pub id: u64,

    pub name: String,

    pub digest: String,

    pub size_bytes: u64,

    pub expired: bool,

    pub workflow_run_id: u64,

    pub workflow_head_branch: String,

    pub workflow_head_sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheProducerContext {
    pub repository: String,

    pub default_branch: String,

    pub git_ref: String,

    pub ref_protected: bool,

    pub workflow_ref: String,

    pub workflow_sha: String,

    pub source_sha: String,

    pub run: QualificationRunRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationCacheReceiptArtifactDocument {
    pub schema: u32,

    pub producer: QualificationCacheProducerContext,

    pub receipt: QualificationCacheReceipt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdmissionNode {
    pub(super) metadata: QualificationCacheRunMetadata,
    pub(super) artifact: QualificationCacheArtifact,
    pub(super) producer: QualificationCacheProducerContext,
    pub(super) receipt: QualificationCacheReceipt,
    #[serde(default)]
    pub(super) previous: Option<Box<AdmissionNode>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AdmissionDocument {
    pub(super) predecessor: AdmissionNode,
    #[serde(default)]
    pub(super) source_delta: Option<QualificationSourceDelta>,
}
