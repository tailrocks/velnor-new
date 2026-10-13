//! Tofu selection wiring tests.
//!
//! Declared via `#[path]` from `select_tofu.rs` under `cfg(test)`.

use std::collections::BTreeSet;
use std::path::Path;

use super::*;
use velnor_actions_contract::{DetectedProject, DetectionStatus};

/// Selected tofu status for `root`.
fn selected(root: &str) -> DetectionStatus {
    DetectionStatus::Selected(DetectedProject {
        stack_id: velnor_actions_tofu_core::STACK_ID.to_owned(),
        project_root: root.to_owned(),
        manifest: root.to_owned(),
    })
}

/// Minimal discovery carrying tofu statuses plus selection records.
fn discovery_with(statuses: Vec<DetectionStatus>, units: Vec<TofuSelectionUnit>) -> Discovery {
    Discovery {
        mise_checks: Vec::new(),
        statuses,
        workspaces: Vec::new(),
        proposals: Vec::new(),
        feature_fallbacks: Vec::new(),
        tool_checks: Vec::new(),
        clippy_memory: crate::clippy_groups::ClippyMemoryPlan {
            groups: Vec::new(),
            barriers: 0,
        },
        recommendations: Vec::new(),
        consumer_manifest_json: None,
        skipped_non_utf8: false,
        tofu_note: None,
        tofu_units: units,
    }
}

mod select_tofu_tests;
