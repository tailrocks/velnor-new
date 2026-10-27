// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "task_report/observation.rs"]
mod observation;
#[path = "task_report/write.rs"]
mod write;

#[path = "task_report_timing.rs"]
mod timing;

pub(crate) use crate::task_report_aggregate::single_task_aggregate;

#[path = "task_report_order.rs"]
mod task_report_order;

pub(crate) use observation::{
    UNIT_OBSERVATION_SUFFIX, UnitObservationExpectation, parse_unit_observation_binding,
    run_attempt, unit_observation_member_name, verify_unit_observation_binding,
    write_entry_reports,
};
pub use write::{REPORT_OP, write_task_report};
pub(crate) use write::{
    TASK_ID_ENV, entry_and_digest, entry_and_digest_for_job, load_plan, terminal_task_report,
};
