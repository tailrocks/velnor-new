// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;
use velnor_actions_contract::{
    PlanGenerator, PlanPreviewChannel, PlanPreviewIdentity, parse_strict_json,
};

use crate::{OrchestratorError, internal::internal};

const IDENTITY_ENV: &str = "VELNOR_SELECTED_PREVIEW_IDENTITY";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SelectedPreviewIdentity {
    channel: String,
    version: String,
    target: String,
    sha256: String,
    source_sha: String,
    receipt_sha256: String,
}

pub(crate) fn captured() -> Result<Option<String>, OrchestratorError> {
    match std::env::var(IDENTITY_ENV) {
        Ok(value) => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(internal("preview_identity_invalid")),
    }
}

pub(crate) fn bind_ambient(generator: &mut PlanGenerator) -> Result<(), OrchestratorError> {
    let evidence = captured()?;
    if let Some(raw) = evidence.as_deref() {
        let identity = parse(raw)?;
        generator.preview = Some(PlanPreviewIdentity {
            channel: PlanPreviewChannel::SourcePreview,
            source_sha: identity.source_sha,
            receipt_sha256: identity.receipt_sha256,
        });
    }
    check(generator, evidence.as_deref())
}

pub(crate) fn check_ambient(generator: &PlanGenerator) -> Result<(), OrchestratorError> {
    check(generator, captured()?.as_deref())
}

pub(crate) fn check(
    generator: &PlanGenerator,
    evidence: Option<&str>,
) -> Result<(), OrchestratorError> {
    let Some(raw) = evidence else {
        return if generator.preview.is_none() {
            Ok(())
        } else {
            Err(internal("preview_controller_identity_missing"))
        };
    };
    let identity = parse(raw)?;
    let target = crate::internal_plan::snapshot::map_release_triple(
        std::env::consts::ARCH,
        std::env::consts::OS,
    );
    if identity.channel != "source-preview"
        || identity.version != env!("CARGO_PKG_VERSION")
        || identity.target != target
        || !lower_hex(&identity.sha256, 64)
        || !lower_hex(&identity.source_sha, 40)
        || !lower_hex(&identity.receipt_sha256, 64)
        || generator.version != identity.version
        || generator.target != identity.target
        || generator.sha256 != identity.sha256
        || generator.preview.as_ref()
            != Some(&PlanPreviewIdentity {
                channel: PlanPreviewChannel::SourcePreview,
                source_sha: identity.source_sha.clone(),
                receipt_sha256: identity.receipt_sha256.clone(),
            })
        || crate::generator::current_exe_sha256().as_deref() != Some(identity.sha256.as_str())
    {
        return Err(internal("preview_controller_identity_mismatch"));
    }
    Ok(())
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn parse(raw: &str) -> Result<SelectedPreviewIdentity, OrchestratorError> {
    if raw.len() > 4096 {
        return Err(internal("preview_identity_invalid"));
    }
    let value = parse_strict_json(raw).map_err(|_| internal("preview_identity_invalid"))?;
    serde_json::from_value(value).map_err(|_| internal("preview_identity_invalid"))
}
