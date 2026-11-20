// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{MbxRemote, RepoFacts};

use crate::error::{DetectError, Reason};
use crate::mise::MISE_CONFIG;

const MBX_TOOL: &str = "mr-boxington";
// First released in tools-v0.1.108 (not tagged yet when this gate was written).
pub(crate) const MBX_PIN_HELPER: (u64, u64, u64) = (0, 1, 108);

#[must_use]
pub fn exact_release(text: &str) -> Option<&str> {
    let bare = text.strip_prefix('v').unwrap_or(text);
    let parts: Vec<&str> = bare.split('.').collect();
    let [major, minor, patch] = parts.as_slice() else {
        return None;
    };
    let number = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|byte| byte.is_ascii_digit())
            && (part == "0" || !part.starts_with('0'))
    };
    (number(major) && number(minor) && number(patch)).then_some(bare)
}

pub(crate) fn check_mbx_pin(facts: &RepoFacts) -> Result<(), DetectError> {
    let reads_pin = facts.config.mbx_remote == MbxRemote::GithubActions
        && facts
            .helper
            .as_ref()
            .is_some_and(|helper| helper.version().at_least(MBX_PIN_HELPER));
    if !reads_pin {
        return Ok(());
    }
    let exact = match facts
        .tools
        .tools
        .get(MBX_TOOL)
        .map(|pin| &pin.requested[..])
    {
        Some([request]) => request.os.is_none() && exact_release(&request.version).is_some(),
        _ => false,
    };
    if exact {
        return Ok(());
    }
    Err(DetectError::invalid(
        facts.tools.configs.first().unwrap_or(&MISE_CONFIG),
        Reason::InvalidValue {
            key: "tools.mr-boxington",
            value: "one exact X.Y.Z release, because the mbx action reads it from mise.toml"
                .to_owned(),
        },
    ))
}
