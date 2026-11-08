// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CrateRegistryRead, CrateReleaseAction, CrateReleaseObservation, GitHubAdmissionRead,
    JournalIdentity, JournalIo, JournalMutation, JournalMutationKind, JournalMutationPhase,
    JournalStage, OrchestratorError, authorize_github_candidate, inspect_journal, invalid,
    observe_admitted_crate_release, verify_binding, verify_observed_packages,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RecoveryState {
    Present,
    Missing,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum PriorRecordState {
    IntentionOnly,
    ObservationRecorded,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RecoveryOperationObservation {
    pub prior_identity: JournalIdentity,
    pub sequence: u32,
    pub package: String,
    pub version: String,
    pub kind: JournalMutationKind,
    pub prior_record: PriorRecordState,
    pub state: RecoveryState,
}

pub(crate) fn observe_prior_recovery<
    J: JournalIo + crate::crate_release_journal::JournalOperationRefs,
    G: GitHubAdmissionRead,
    R: CrateRegistryRead,
>(
    input: &CrateReleaseObservation<'_, J, G, R>,
    previous: &JournalIdentity,
) -> Result<Vec<RecoveryOperationObservation>, OrchestratorError> {
    verify_binding(input)?;
    authorize_github_candidate(input.request, &input.admission.github, input.github)?;
    let started = inspect_journal(input.journal, input.identity, JournalStage::Started)?
        .ok_or_else(invalid)?;
    if started.receipt != *input.journal_receipt {
        return Err(invalid());
    }
    if input.journal_receipt.predecessor.as_ref() != Some(previous) {
        return Err(invalid());
    }
    let deadline = velnor_actions_mise::CheckDeadline::after(std::time::Duration::from_secs(600))
        .map_err(|_| invalid())?
        .effective();
    let context = crate::release::crate_release_direct_history::HistoryContext::admitted(
        input.source,
        input.intent,
        input.request,
        input.git,
        input.journal_receipt,
        deadline,
    )?;
    let edge = crate::crate_release_journal::PriorTransition {
        context: &context,
        github: input.github,
    };
    let mut pending = Vec::new();
    let history = crate::crate_release_journal::walk_prior_history_until(
        input.journal,
        input.journal_receipt,
        input.source,
        input.git,
        Some(&edge),
        deadline,
        |prior| {
            collect_intentions(&mut pending, &prior);
            Ok(())
        },
    )?;
    let current = observe_admitted_crate_release(
        input.intent,
        input.request,
        input.github,
        input.registry,
        crate::CrateSource::new(input.source, input.git, input.source_execution),
        &input.admission.github,
    )?;
    verify_observed_packages(&current, input.admission)?;
    let results = pending
        .into_iter()
        .map(|item| item.observe(&current))
        .collect::<Result<Vec<_>, _>>()?;
    if observe_admitted_crate_release(
        input.intent,
        input.request,
        input.github,
        input.registry,
        crate::CrateSource::new(input.source, input.git, input.source_execution),
        &input.admission.github,
    )? != current
        || !crate::crate_release_journal::walk_prior_history_until(
            input.journal,
            input.journal_receipt,
            input.source,
            input.git,
            Some(&edge),
            deadline,
            |_| Ok(()),
        )?
        .matches(&history)?
    {
        return Err(invalid());
    }
    authorize_github_candidate(input.request, &input.admission.github, input.github)?;
    Ok(results)
}

struct HistoricalIntention {
    identity: JournalIdentity,
    sequence: u32,
    package: String,
    version: String,
    kind: JournalMutationKind,
    prior_record: PriorRecordState,
}

fn collect_intentions(
    pending: &mut Vec<HistoricalIntention>,
    prior: &crate::crate_release_journal::PriorHistoryEvidence,
) {
    for (index, operation) in prior.mutations.iter().enumerate() {
        if matches!(operation.phase, JournalMutationPhase::Intended) {
            pending.push(HistoricalIntention {
                identity: prior.start.receipt.identity.clone(),
                sequence: operation.sequence,
                package: operation.package.clone(),
                version: operation.version.clone(),
                kind: operation.kind,
                prior_record: prior_record_state(prior.mutations.get(index + 1)),
            });
        }
    }
}

impl HistoricalIntention {
    fn observe(
        self,
        current: &super::super::CrateReleaseAdmissionReceipt,
    ) -> Result<RecoveryOperationObservation, OrchestratorError> {
        let package = current
            .packages
            .iter()
            .find(|package| {
                package.source.name == self.package && package.source.version == self.version
            })
            .ok_or_else(invalid)?;
        Ok(RecoveryOperationObservation {
            prior_identity: self.identity,
            sequence: self.sequence,
            package: self.package,
            version: self.version,
            kind: self.kind,
            prior_record: self.prior_record,
            state: recovery_state(self.kind, &package.action),
        })
    }
}

pub(super) fn prior_record_state(next: Option<&JournalMutation>) -> PriorRecordState {
    if next.is_some_and(|item| matches!(item.phase, JournalMutationPhase::Observed { .. })) {
        PriorRecordState::ObservationRecorded
    } else {
        PriorRecordState::IntentionOnly
    }
}

pub(super) fn recovery_state(
    kind: JournalMutationKind,
    action: &CrateReleaseAction,
) -> RecoveryState {
    match (kind, action) {
        (JournalMutationKind::Upload, CrateReleaseAction::Publish)
        | (JournalMutationKind::CreateTag, CrateReleaseAction::RepairTag { .. })
        | (JournalMutationKind::CreateRelease, CrateReleaseAction::RepairRelease { .. }) => {
            RecoveryState::Missing
        }
        (JournalMutationKind::CreateTag, CrateReleaseAction::Publish)
        | (
            JournalMutationKind::CreateRelease,
            CrateReleaseAction::Publish | CrateReleaseAction::RepairTag { .. },
        ) => RecoveryState::Blocked,
        _ => RecoveryState::Present,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crate_release_journal::JournalMutationResult;

    #[test]
    fn recovery_state_respects_operation_prerequisites() {
        let registry = crate::crate_release_registry::CrateRegistryReconciliation {
            checksum: "d".repeat(64),
            downloaded_checksum: "d".repeat(64),
            owners: Vec::new(),
            features: std::collections::BTreeMap::new(),
        };
        let actions = [
            CrateReleaseAction::Publish,
            CrateReleaseAction::RepairTag {
                registry: registry.clone(),
            },
            CrateReleaseAction::RepairRelease {
                registry: registry.clone(),
            },
            CrateReleaseAction::Reconciled {
                registry,
                release: crate::crate_release_github::GitHubReleaseEvidence {
                    id: 1,
                    url: "https://github.com/example/demo/releases/tag/demo-v0.1.1".into(),
                },
            },
        ];
        let expected = [
            (
                JournalMutationKind::Upload,
                [
                    RecoveryState::Missing,
                    RecoveryState::Present,
                    RecoveryState::Present,
                    RecoveryState::Present,
                ],
            ),
            (
                JournalMutationKind::CreateTag,
                [
                    RecoveryState::Blocked,
                    RecoveryState::Missing,
                    RecoveryState::Present,
                    RecoveryState::Present,
                ],
            ),
            (
                JournalMutationKind::CreateRelease,
                [
                    RecoveryState::Blocked,
                    RecoveryState::Blocked,
                    RecoveryState::Missing,
                    RecoveryState::Present,
                ],
            ),
        ];
        for (kind, states) in expected {
            for (action, state) in actions.iter().zip(states) {
                assert_eq!(recovery_state(kind, action), state);
            }
        }
    }

    #[test]
    fn prior_record_state_distinguishes_recorded_observation_from_pending_intent() {
        let observed = JournalMutation {
            archive_sha256: None,
            semver_evidence: None,
            sequence: 2,
            package: "demo".into(),
            version: "0.1.1".into(),
            kind: JournalMutationKind::CreateTag,
            phase: JournalMutationPhase::Observed {
                result: JournalMutationResult::Tag {
                    name: "demo-v0.1.1".into(),
                    target_sha: "a".repeat(40),
                },
            },
        };
        assert_eq!(prior_record_state(None), PriorRecordState::IntentionOnly);
        assert_eq!(
            prior_record_state(Some(&observed)),
            PriorRecordState::ObservationRecorded
        );
    }
}
