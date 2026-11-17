// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{CacheKey, Step, StepKind};

pub const BASE_PREFIX_OUTPUT: &str = "base_prefix";
pub const BASE_SAVE_OUTPUT: &str = "base_save";
pub const SOURCES_PREFIX_OUTPUT: &str = "sources_prefix";
pub const SOURCES_SAVED_OUTPUT: &str = "sources_saved";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheLaneOutput {
    BasePrefix(CacheKey),
    BaseSave,
    SourcesPrefix(CacheKey),
    SourcesSaved(CacheKey),
    CacheOk(Vec<CacheKey>),
}

impl CacheLaneOutput {
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::BasePrefix(_) => BASE_PREFIX_OUTPUT,
            Self::BaseSave => BASE_SAVE_OUTPUT,
            Self::SourcesPrefix(_) => SOURCES_PREFIX_OUTPUT,
            Self::SourcesSaved(_) => SOURCES_SAVED_OUTPUT,
            Self::CacheOk(_) => "cache_ok",
        }
    }

    #[must_use]
    pub fn max_content_bytes(&self) -> usize {
        match self {
            Self::BaseSave | Self::SourcesSaved(_) | Self::CacheOk(_) => 5,
            Self::BasePrefix(key) | Self::SourcesPrefix(key) => {
                let layout = key.layout();
                let Some(&length) = layout.fallback_lengths.last() else {
                    return usize::MAX;
                };
                layout.segments[..length]
                    .iter()
                    .fold(0_usize, |bytes, segment| {
                        bytes
                            .saturating_add(segment.json_content_max_bytes())
                            .saturating_add(1)
                    })
            }
        }
    }
}

#[must_use]
pub fn cache_lane_outputs(steps: &[Step]) -> Vec<CacheLaneOutput> {
    let decides = steps
        .iter()
        .any(|step| matches!(step.kind, StepKind::CacheDecide { .. }));
    let saved = |wanted: fn(&CacheKey) -> bool| {
        steps.iter().find_map(|step| match &step.kind {
            StepKind::CacheSave(key) if wanted(key) => Some(key.clone()),
            _ => None,
        })
    };
    let target = saved(|key| matches!(key, CacheKey::Target { .. })).filter(|_| decides);
    let sources = saved(|key| matches!(key, CacheKey::Sources { .. })).filter(|_| decides);
    let verified = steps
        .iter()
        .filter_map(|step| match &step.kind {
            StepKind::CacheSave(key @ (CacheKey::Target { .. } | CacheKey::Sources { .. })) => {
                Some(key.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    target
        .into_iter()
        .flat_map(|key| [CacheLaneOutput::BasePrefix(key), CacheLaneOutput::BaseSave])
        .chain((!verified.is_empty()).then_some(CacheLaneOutput::CacheOk(verified)))
        .chain(sources.into_iter().flat_map(|key| {
            [
                CacheLaneOutput::SourcesPrefix(key.clone()),
                CacheLaneOutput::SourcesSaved(key),
            ]
        }))
        .collect()
}
