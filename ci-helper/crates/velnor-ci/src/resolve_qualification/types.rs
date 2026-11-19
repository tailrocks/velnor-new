// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The records the legacy `resolve-qualification-v1` op reads and writes. Field names, field
//! order, `deny_unknown_fields` and the `null`s a missing option writes are the legacy ones, so
//! the canonical bytes of the admission file are the same.

use serde::{Deserialize, Serialize};

pub(super) const RECEIPT_ARTIFACT: &str = "velnor-qualification-cache-receipt-v2";
pub(super) const RECEIPT_FILENAME: &str = "qualification-cache-receipt.json";
pub(super) const MAX_RECEIPT_BYTES: usize = 524_288;
pub(super) const MAX_DEPTH: usize = 4;
pub(super) const MAX_LANES: usize = 256;
pub(super) const CI_WORKFLOW_PATH: &str = ".github/workflows/ci.yml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Phase {
    Cold,
    Warm,
    Third,
    UsefulDelta,
    Control,
}

impl Phase {
    pub(super) const fn predecessor(self) -> Option<Self> {
        match self {
            Self::Cold | Self::Control => None,
            Self::Warm => Some(Self::Cold),
            Self::Third => Some(Self::Warm),
            Self::UsefulDelta => Some(Self::Third),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RunRef {
    pub(super) run_id: u64,
    pub(super) run_attempt: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Dispatch {
    pub(super) campaign: String,
    pub(super) phase: Phase,
    pub(super) repository: String,
    pub(super) default_branch: String,
    pub(super) git_ref: String,
    pub(super) ref_protected: bool,
    pub(super) workflow_ref: String,
    pub(super) workflow_sha: String,
    pub(super) source_sha: String,
    pub(super) run_id: u64,
    pub(super) run_attempt: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) predecessor: Option<RunRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Restore {
    Miss,
    Hit,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum SaveAction {
    Succeeded,
    Skipped,
    NotRequired,
    Failed,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct BackendEntry {
    pub(super) id: u64,
    pub(super) key: String,
    pub(super) git_ref: String,
    pub(super) size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "entry", rename_all = "snake_case")]
pub(super) enum Observation {
    Absent,
    Found(BackendEntry),
    NotQueried,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RestoreRecord {
    pub(super) requested_key: Option<String>,
    pub(super) matched_key: Option<String>,
    pub(super) matched_cache: Observation,
    pub(super) result: Restore,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SaveRecord {
    pub(super) requested_key: Option<String>,
    pub(super) action: SaveAction,
    pub(super) before: Observation,
    pub(super) after: Observation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Layer {
    MbxObjects,
    CargoSources,
    MiseTools,
    TofuProviders,
    TaskResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimePlatform {
    pub(super) os: String,
    pub(super) arch: String,
    pub(super) runs_on: String,
    pub(super) image_os: String,
    pub(super) target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RuntimeIdentity {
    pub(super) platform: RuntimePlatform,
    pub(super) toolchain_id: String,
    pub(super) cache_format_id: String,
    pub(super) abi_id: String,
    pub(super) driver_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LayerReceipt {
    pub(super) layer: Layer,
    pub(super) active: bool,
    pub(super) identity_digest: String,
    pub(super) runtime_identity: Option<RuntimeIdentity>,
    pub(super) state_digest: Option<String>,
    pub(super) restore: RestoreRecord,
    pub(super) save: SaveRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LaneReceipt {
    pub(super) matrix_key: String,
    pub(super) stack_id: String,
    pub(super) task_id: String,
    pub(super) completed_task_ids: Vec<String>,
    pub(super) closure_digest: String,
    pub(super) useful_state_digest: String,
    pub(super) layers: Vec<LayerReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReceiptLink {
    pub(super) run: RunRef,
    pub(super) receipt_digest: String,
    pub(super) artifact_id: u64,
    pub(super) artifact_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SourceDelta {
    pub(super) base_source_sha: String,
    pub(super) source_sha: String,
    pub(super) changed_paths: Vec<String>,
    pub(super) diff_digest: String,
    pub(super) base_is_ancestor: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    pub(super) schema: u32,
    pub(super) plan_id: String,
    pub(super) run: RunRef,
    pub(super) campaign: String,
    pub(super) phase: Phase,
    pub(super) source_sha: String,
    pub(super) configuration_digest: String,
    #[serde(default)]
    pub(super) source_delta: Option<SourceDelta>,
    pub(super) predecessor: Option<ReceiptLink>,
    pub(super) lanes: Vec<LaneReceipt>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RunMetadata {
    pub(super) repository: String,
    pub(super) default_branch: String,
    pub(super) git_ref: String,
    pub(super) ref_protected: bool,
    pub(super) workflow_path_ref: String,
    pub(super) workflow_ref: String,
    pub(super) workflow_sha: String,
    pub(super) head_sha: String,
    pub(super) event: String,
    pub(super) conclusion: String,
    pub(super) run: RunRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Artifact {
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) digest: String,
    pub(super) size_bytes: u64,
    pub(super) expired: bool,
    pub(super) workflow_run_id: u64,
    pub(super) workflow_head_branch: String,
    pub(super) workflow_head_sha: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Producer {
    pub(super) repository: String,
    pub(super) default_branch: String,
    pub(super) git_ref: String,
    pub(super) ref_protected: bool,
    pub(super) workflow_ref: String,
    pub(super) workflow_sha: String,
    pub(super) source_sha: String,
    pub(super) run: RunRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReceiptDocument {
    pub(super) schema: u32,
    pub(super) producer: Producer,
    pub(super) receipt: Receipt,
}

/// One run of the lineage: what the API said of it, its receipt, and the run before it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Node {
    pub(super) metadata: RunMetadata,
    pub(super) artifact: Artifact,
    pub(super) producer: Producer,
    pub(super) receipt: Receipt,
    #[serde(default)]
    pub(super) previous: Option<Box<Node>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Admission {
    pub(super) predecessor: Node,
    #[serde(default)]
    pub(super) source_delta: Option<SourceDelta>,
}
