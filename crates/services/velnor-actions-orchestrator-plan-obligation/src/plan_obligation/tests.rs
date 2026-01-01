//! Obligation extension-dispatch tests.
//!
//! Declared via `#[path]` from `plan_obligation.rs` under `cfg(test)`.

use std::path::PathBuf;

use super::*;
use velnor_actions_contract::cachekey::{RUST_EXTENSION_SCHEMA, TOFU_EXTENSION_SCHEMA};

/// Minimal discovery with no workspaces or tool checks.
fn empty_discovery() -> Discovery {
    Discovery {
        mise_checks: Vec::new(),
        statuses: Vec::new(),
        workspaces: Vec::new(),
        proposals: Vec::new(),
        feature_fallbacks: Vec::new(),
        tool_checks: Vec::new(),
        clippy_memory: velnor_actions_orchestrator_core::clippy_groups::ClippyMemoryPlan {
            groups: Vec::new(),
            barriers: 0,
        },
        recommendations: Vec::new(),
        consumer_manifest_json: None,
        skipped_non_utf8: false,
        tofu_note: None,
        tofu_units: Vec::new(),
    }
}

/// Tofu proposal via the T12 adapter constructor.
fn tofu_proposal(kind: velnor_actions_tofu_core::TofuTaskKind) -> ProposedTask {
    let group = velnor_actions_tofu_core::TofuTaskGroup {
        root: String::new(),
        kind,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let task = velnor_actions_tofu_core::propose_task(&group).expect("fixture proposes");
    task.validate().expect("fixture valid");
    task
}

/// Unique temporary directory removed on drop.
struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn create(label: &str) -> std::io::Result<Self> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let path = std::env::temp_dir().join(format!(
            "velnor-plan-obligation-{label}-{nanos}-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self { path })
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.path));
    }
}

mod plan_obligation_tests;
