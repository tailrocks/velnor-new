// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{MBX_ACTION_RELEASE, MbxRemote, RepoFacts};

use crate::error::{DetectError, Reason};
use crate::mise::MISE_CONFIG;

pub(crate) const MBX_TOOL: &str = "mr-boxington";
pub(crate) const MBX_PIN_HELPER: (u64, u64, u64) = (0, 1, 109);

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
    if facts.config.mbx_remote != MbxRemote::GithubActions {
        return Ok(());
    }
    let requests = facts
        .tools
        .tools
        .get(MBX_TOOL)
        .map(|pin| &pin.requested[..]);
    let (major, minor, patch) = MBX_PIN_HELPER;
    if facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least(MBX_PIN_HELPER))
    {
        let exact = match requests {
            Some([request]) => request.os.is_none() && exact_release(&request.version).is_some(),
            _ => false,
        };
        if exact {
            return Ok(());
        }
        return Err(pin_error(
            facts,
            "one exact X.Y.Z release, because the mbx action reads it from mise.toml".to_owned(),
        ));
    }
    match requests {
        None | Some([]) => Ok(()),
        Some([request])
            if request.os.is_none()
                && exact_release(&request.version) == Some(MBX_ACTION_RELEASE) =>
        {
            Ok(())
        }
        Some(pins) => {
            let pinned: Vec<&str> = pins.iter().map(|pin| pin.version.as_str()).collect();
            Err(pin_error(
                facts,
                format!(
                    "{MBX_ACTION_RELEASE}, the only release a velnor-ci helper before {major}.{minor}.{patch} installs (this pin is {}); pin mr-boxington to {MBX_ACTION_RELEASE} or pin velnor-ci {major}.{minor}.{patch} or later",
                    pinned.join(", ")
                ),
            ))
        }
    }
}

fn pin_error(facts: &RepoFacts, value: String) -> DetectError {
    DetectError::invalid(
        facts.tools.configs.first().unwrap_or(&MISE_CONFIG),
        Reason::InvalidValue {
            key: "tools.mr-boxington",
            value,
        },
    )
}
