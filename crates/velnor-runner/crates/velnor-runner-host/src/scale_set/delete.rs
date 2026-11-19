// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_runner_github::{
    ScaleSetById, ScaleSetByName, ScaleSetDelete, ScaleSetDeleteStatus, ScaleSetFound,
    ScaleSetObserved, Transport, delete_runner_scale_set, get_runner_scale_set,
    observe_runner_scale_set_by_id,
};

use crate::error::HostError;
use crate::journal::{
    ConnectionJournal, ScaleSetDeleteClaim, ScaleSetDeleteGate, ScaleSetDeleteMode,
    ScaleSetDeleteRecord, ScaleSetDeleteTarget, ScaleSetDeleted,
};

#[derive(Debug)]
pub(crate) struct ScaleSetAbsence(());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScaleSetDeleteOutcome {
    Deleted,

    NotOwned,

    NotDrained,

    Refused(u16),

    Uncertain,
}

pub(crate) async fn run_created_delete<T: Transport + ?Sized>(
    transport: &mut T,
    admin_token: &str,
    scoped: &ConnectionJournal,
) -> Result<ScaleSetDeleteOutcome, HostError> {
    let target = match scoped.scale_set_delete_gate().await? {
        ScaleSetDeleteGate::NotOwned => return Ok(ScaleSetDeleteOutcome::NotOwned),
        ScaleSetDeleteGate::NotDrained => return Ok(ScaleSetDeleteOutcome::NotDrained),
        ScaleSetDeleteGate::Deleted => return Ok(ScaleSetDeleteOutcome::Deleted),
        ScaleSetDeleteGate::Owned(target) => target,
    };
    let claim = scoped.claim_scale_set_delete(&target).await?;
    if claim.mode() == ScaleSetDeleteMode::Done {
        return Ok(ScaleSetDeleteOutcome::Deleted);
    }
    let resend = claim.mode() == ScaleSetDeleteMode::Observe;
    match observe(transport, admin_token, &target) {
        Observation::Absent(proof) => {
            let deleted = ScaleSetDeleted::after_proven_absence(&proof);
            settle(scoped, &claim, ScaleSetDeleteRecord::Deleted(deleted)).await?;
            Ok(ScaleSetDeleteOutcome::Deleted)
        }
        Observation::Unknown => {
            settle(scoped, &claim, ScaleSetDeleteRecord::Uncertain).await?;
            Ok(ScaleSetDeleteOutcome::Uncertain)
        }
        Observation::Present => {
            if resend {
                settle(scoped, &claim, ScaleSetDeleteRecord::Resend).await?;
            }
            send(transport, admin_token, scoped, &claim, &target).await
        }
    }
}

pub(crate) async fn recover_intended_delete<T: Transport + ?Sized>(
    transport: &mut T,
    admin_token: &str,
    scoped: &ConnectionJournal,
) -> Result<Option<ScaleSetDeleteOutcome>, HostError> {
    if !scoped.has_intended_scale_set_delete().await? {
        return Ok(None);
    }
    run_created_delete(transport, admin_token, scoped)
        .await
        .map(Some)
}

async fn send<T: Transport + ?Sized>(
    transport: &mut T,
    admin_token: &str,
    scoped: &ConnectionJournal,
    claim: &ScaleSetDeleteClaim,
    target: &ScaleSetDeleteTarget,
) -> Result<ScaleSetDeleteOutcome, HostError> {
    let call = ScaleSetDelete {
        scale_set_id: target.scale_set_id,
        admin_token,
    };
    let (record, outcome) = match delete_runner_scale_set(transport, &call) {
        Ok(ScaleSetDeleteStatus::Deleted) => (
            ScaleSetDeleteRecord::Deleted(ScaleSetDeleted::after_http_204()),
            ScaleSetDeleteOutcome::Deleted,
        ),
        Ok(ScaleSetDeleteStatus::Refused(status)) => (
            ScaleSetDeleteRecord::Refused,
            ScaleSetDeleteOutcome::Refused(status),
        ),
        Ok(ScaleSetDeleteStatus::NotFound) => match absence(transport, admin_token, target) {
            Some(proof) => (
                ScaleSetDeleteRecord::Deleted(ScaleSetDeleted::after_proven_absence(&proof)),
                ScaleSetDeleteOutcome::Deleted,
            ),
            None => (
                ScaleSetDeleteRecord::Uncertain,
                ScaleSetDeleteOutcome::Uncertain,
            ),
        },
        Err(_) => (
            ScaleSetDeleteRecord::Uncertain,
            ScaleSetDeleteOutcome::Uncertain,
        ),
    };
    settle(scoped, claim, record).await?;
    Ok(outcome)
}

async fn settle(
    scoped: &ConnectionJournal,
    claim: &ScaleSetDeleteClaim,
    record: ScaleSetDeleteRecord,
) -> Result<(), HostError> {
    scoped.record_scale_set_delete(claim, record).await
}

enum Observation {
    Present,
    Absent(ScaleSetAbsence),
    Unknown,
}

fn observe<T: Transport + ?Sized>(
    transport: &mut T,
    admin_token: &str,
    target: &ScaleSetDeleteTarget,
) -> Observation {
    match observe_runner_scale_set_by_id(transport, &by_id(admin_token, target)) {
        Ok(ScaleSetObserved::Found(view)) if view.id == target.scale_set_id => Observation::Present,
        Ok(ScaleSetObserved::Missing) => absent_from_group(transport, admin_token, target)
            .map_or(Observation::Unknown, Observation::Absent),
        Ok(ScaleSetObserved::Found(_)) | Err(_) => Observation::Unknown,
    }
}

fn absence<T: Transport + ?Sized>(
    transport: &mut T,
    admin_token: &str,
    target: &ScaleSetDeleteTarget,
) -> Option<ScaleSetAbsence> {
    if !matches!(
        observe_runner_scale_set_by_id(transport, &by_id(admin_token, target)),
        Ok(ScaleSetObserved::Missing)
    ) {
        return None;
    }
    absent_from_group(transport, admin_token, target)
}

fn absent_from_group<T: Transport + ?Sized>(
    transport: &mut T,
    admin_token: &str,
    target: &ScaleSetDeleteTarget,
) -> Option<ScaleSetAbsence> {
    let listed = get_runner_scale_set(
        transport,
        &ScaleSetByName {
            runner_group_id: target.runner_group_id,
            name: &target.name,
            expected_labels: &target.labels,
            admin_token,
        },
    );
    match listed {
        Ok(ScaleSetFound::NotFound) => Some(ScaleSetAbsence(())),
        Ok(ScaleSetFound::Found(view)) if view.id != target.scale_set_id => {
            Some(ScaleSetAbsence(()))
        }
        Ok(ScaleSetFound::Found(_)) | Err(_) => None,
    }
}

fn by_id<'a>(admin_token: &'a str, target: &'a ScaleSetDeleteTarget) -> ScaleSetById<'a> {
    ScaleSetById {
        scale_set_id: target.scale_set_id,
        name: &target.name,
        expected_labels: &target.labels,
        admin_token,
    }
}

#[cfg(test)]
pub(crate) mod tests;
