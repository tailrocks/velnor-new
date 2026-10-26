// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

#[derive(Clone, Copy)]
pub(super) enum Phase {
    Discovery,
    Qualification,
    QualificationComplete,
    Task,
    TaskComplete,
    SourceRecheckComplete,
    Artifacts,
    ArtifactsComplete,
}

pub(super) struct Progress {
    started: Instant,
    scope: &'static str,
}

impl Progress {
    pub(super) fn producer() -> Self {
        Self {
            started: Instant::now(),
            scope: "producer",
        }
    }

    pub(super) fn artifacts() -> Self {
        Self {
            started: Instant::now(),
            scope: "artifacts",
        }
    }

    pub(super) fn mark(&self, phase: Phase) {
        let phase = match phase {
            Phase::Discovery => "discovery",
            Phase::Qualification => "qualification",
            Phase::QualificationComplete => "qualification-complete",
            Phase::Task => "task",
            Phase::TaskComplete => "task-complete",
            Phase::SourceRecheckComplete => "source-recheck-complete",
            Phase::Artifacts => "verification",
            Phase::ArtifactsComplete => "verification-complete",
        };
        eprintln!(
            "velnor-preview {} phase={phase} elapsed_ms={}",
            self.scope,
            self.started.elapsed().as_millis()
        );
    }
}
