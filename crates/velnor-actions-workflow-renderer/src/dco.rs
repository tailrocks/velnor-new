// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::{DCO_CONFIG_PATH, DcoConfig};

use crate::RenderError;
use crate::marker::with_marker;
use crate::render::RenderedFile;

pub fn render_dco_yml(config: &DcoConfig, version: &str) -> Result<RenderedFile, RenderError> {
    let mut lines: Vec<String> = Vec::new();
    if let Some(allow) = config.allow_override_action {
        lines.push(format!("allowOverrideAction: {allow}"));
    }
    if let Some(remediation) = &config.allow_remediation_commits {
        lines.push("allowRemediationCommits:".to_owned());
        if let Some(individual) = remediation.individual {
            lines.push(format!("  individual: {individual}"));
        }
        if let Some(third_party) = remediation.third_party {
            lines.push(format!("  thirdParty: {third_party}"));
        }
    }
    if let Some(require) = &config.require {
        lines.push("require:".to_owned());
        lines.push(format!("  members: {}", require.members));
    }
    let body = lines.join("\n") + "\n";
    Ok(RenderedFile {
        path: DCO_CONFIG_PATH.to_owned(),
        bytes: with_marker(version, &body)?,
    })
}
