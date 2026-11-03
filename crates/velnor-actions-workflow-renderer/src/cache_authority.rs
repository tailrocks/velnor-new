// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::workflow::{
    QUALIFICATION_WRITER_GATE, WORKFLOW_CACHE_MODE, WRITER_CACHE_MODE,
};

use crate::{RenderError, yaml::Yaml};

pub const WORKFLOW_CACHE_MODE_KEY: &str = "cache-mode";

pub(crate) fn bind_workflow_cache_mode(path: &str, document: &mut Yaml) -> Result<(), RenderError> {
    if !crate::workflow_size::is_workflow_path(path) {
        return Ok(());
    }
    let Yaml::Map(entries) = document else {
        return Err(invalid(path));
    };
    if let Some((_, value)) = entries
        .iter()
        .find(|(key, _)| key == WORKFLOW_CACHE_MODE_KEY)
    {
        return match value {
            Yaml::Str(mode) if mode == WORKFLOW_CACHE_MODE => Ok(()),
            _ => Err(invalid(path)),
        };
    }
    let at = entries
        .iter()
        .position(|(key, _)| key == "permissions")
        .map_or(entries.len().min(2), |index| index + 1);
    entries.insert(
        at,
        (
            WORKFLOW_CACHE_MODE_KEY.to_owned(),
            Yaml::str(WORKFLOW_CACHE_MODE),
        ),
    );
    Ok(())
}

fn invalid(path: &str) -> RenderError {
    RenderError::InvalidWorkflow(format!("workflow_cache_mode_not_read:{path}"))
}

pub(crate) fn qualification_writer_field() -> (String, Yaml) {
    (
        WORKFLOW_CACHE_MODE_KEY.to_owned(),
        Yaml::str(WRITER_CACHE_MODE),
    )
}

pub(crate) fn qualification_branch_gate(mode_condition: &str) -> String {
    format!("{mode_condition} && {QUALIFICATION_WRITER_GATE}")
}
