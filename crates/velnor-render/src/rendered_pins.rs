// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::helper_steps::MBX_ACTION_SHA;
use crate::pinned_actions::PINNED_ACTIONS;
use crate::release_steps::{APP_TOKEN_COMMIT, APP_TOKEN_VERSION};
use crate::rules::{APP_TOKEN_ACTION, MBX_ACTION};
use crate::steps::{CACHE_RESTORE, CACHE_SAVE, CHECKOUT, DOWNLOAD_ARTIFACT, MISE, UPLOAD_ARTIFACT};

/// One third-party action commit the renderer emits. `tag` is the version the
/// renderer records next to the commit, or `None` when it records only the commit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderedPin {
    pub action: &'static str,
    pub commit: &'static str,
    pub tag: Option<&'static str>,
}

/// Every third-party action pin the renderer can emit. The guard test in
/// `velnor-gen` compares each entry with `.velnor/version-policy.toml`, and
/// checks that no 40-character commit literal in this crate is missing here.
#[must_use]
pub fn rendered_pins() -> Vec<RenderedPin> {
    let tagged = PINNED_ACTIONS.iter().map(|pin| RenderedPin {
        action: pin.action,
        commit: pin.commit,
        tag: Some(pin.tag),
    });
    let commit_only = [
        CHECKOUT,
        MISE,
        UPLOAD_ARTIFACT,
        DOWNLOAD_ARTIFACT,
        CACHE_RESTORE,
        CACHE_SAVE,
    ]
    .into_iter()
    .map(|uses| split_uses(uses, None));
    let extra = [
        RenderedPin {
            action: APP_TOKEN_ACTION,
            commit: APP_TOKEN_COMMIT,
            tag: Some(APP_TOKEN_VERSION),
        },
        RenderedPin {
            action: MBX_ACTION,
            commit: MBX_ACTION_SHA,
            tag: Some("v1.7.3"),
        },
    ];
    tagged.chain(commit_only).chain(extra).collect()
}

fn split_uses(uses: &'static str, tag: Option<&'static str>) -> RenderedPin {
    let (action, commit) = uses.split_once('@').unwrap_or((uses, ""));
    RenderedPin {
        action,
        commit,
        tag,
    }
}
