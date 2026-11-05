// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, path::Path};

pub(super) fn reviewed_non_inputs() -> Result<String, Box<dyn Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("workspace root missing")?;
    reviewed_non_inputs_from(root)
}

pub(super) fn reviewed_non_inputs_from(root: &Path) -> Result<String, Box<dyn Error>> {
    let config = velnor_actions_orchestrator::prepare(root)?.config;
    let patterns = config
        .stacks
        .rust
        .as_ref()
        .map_or(&[][..], |rust| rust.non_inputs.globs());
    Ok(serde_json::to_string(patterns)?)
}
