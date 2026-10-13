//! Reuse-stage tests.
//!
//! Declared via `#[path]` from `reuse_stages.rs` under `cfg(test)`.

use super::*;
use std::collections::BTreeMap;
use velnor_actions_contract::digest_b3;

/// Bound task for every identity fixture.
const TASK_ID: &str = "stack/rust/root/clippy/default";

/// Complete descriptor over one source, output, env entry, and tool.
fn descriptor() -> CachedTaskDescriptor {
    CachedTaskDescriptor {
        task_name: "clippy".to_owned(),
        sources: vec!["Cargo.toml".to_owned()],
        outputs: vec!["out/report.json".to_owned()],
        command_inputs: Vec::new(),
        env: BTreeMap::from([("RUSTFLAGS".to_owned(), "--deny warnings".to_owned())]),
        tools: vec!["rust@1.98.1".to_owned()],
        dep_keys: Vec::new(),
    }
}

/// Expected identity plus matching restore metadata and live digest.
fn identity() -> (ExpectedReuseIdentity, ObservedRestoreMeta, String) {
    let key = digest_b3(b"key");
    let compat = digest_b3(b"compat");
    let inputs = digest_b3(b"inputs");
    (
        ExpectedReuseIdentity {
            cache_key: key.clone(),
            compatibility_id: compat.clone(),
            owner_scope: "trusted".to_owned(),
            input_digest: inputs.clone(),
        },
        ObservedRestoreMeta {
            task_id: TASK_ID.to_owned(),
            key,
            compat,
            owner: "trusted".to_owned(),
        },
        inputs,
    )
}

/// Declared outputs plus matching byte-verified observations.
fn outputs() -> (Vec<String>, Vec<ObservedOutput>) {
    let bytes = b"report-bytes".to_vec();
    let digest = digest_b3(&bytes);
    (
        vec!["out/report.json".to_owned()],
        vec![("out/report.json".to_owned(), bytes, digest)],
    )
}

mod reuse_stages_tests;
