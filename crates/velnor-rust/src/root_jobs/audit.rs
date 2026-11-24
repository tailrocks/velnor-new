// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperCommand, Step, StepKind};

use super::RootJobs;
use crate::audit_job::{
    AuditLocation, audit_gate, audit_when, audits_before_saves, audits_per_root,
};
use crate::plan::step;

impl RootJobs<'_> {
    pub(super) fn audit(&self) -> Option<Step> {
        if self.audit == AuditLocation::Separate {
            return None;
        }
        let when = audit_when(self.facts)?;
        let kind = if audits_per_root(self.facts) {
            StepKind::InputsAuditRoot(
                self.unit
                    .roots
                    .first()
                    .and_then(|root| root.manifest.parent()),
            )
        } else {
            StepKind::Helper(HelperCommand::InputsAudit)
        };
        Some(Step {
            when,
            gate: audit_gate(self.facts, self.lane),
            ..step(kind)
        })
    }

    pub(super) fn audit_after_saves(&self) -> bool {
        !audits_before_saves(self.facts)
    }
}
