// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "merge_checks/agreement.rs"]
mod agreement;
#[path = "merge_checks/ledgers.rs"]
mod ledgers;
#[path = "merge_checks/reports.rs"]
mod reports;

pub(crate) use agreement::{
    CandidateAttestation, check_agreement, check_candidate_binding, check_execute_inventory,
    check_plan_shape, check_trust_coherence,
};
pub(crate) use ledgers::check_group_ledgers;
pub(crate) use reports::{
    check_plan_evidence, expected_task_reports, partition_task_reports, plan_digests, plan_entries,
    shards_failed,
};
