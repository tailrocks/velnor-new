// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::pinned_actions::ACTIONS_CACHE;
use crate::rules::RenderedStep;

pub(crate) const MESSAGE: &str = "actions/cache saves in a post step that runs only when every step succeeded; a later failing step discards the cache, so make it the last step of the job or use actions/cache/restore and actions/cache/save";

pub(crate) fn before_last(step: &RenderedStep, index: usize, last: usize) -> bool {
    index != last
        && step
            .uses
            .as_deref()
            .is_some_and(|uses| uses.split('@').next() == Some(ACTIONS_CACHE.action))
}
