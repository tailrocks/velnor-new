// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "capacity_header/ledger.rs"]
mod ledger;
#[path = "capacity_header/model.rs"]
mod model;
#[path = "capacity_header/storage.rs"]
mod storage;

pub(crate) use ledger::HeaderLedger;

pub(crate) use model::{
    HeaderAllocation, HeaderDeleteAttempt, HeaderLedgerError, HeaderLedgerSnapshot,
    HeaderPollAttempt, PollOutcome,
};

pub(super) use storage::{
    bootstrap, bootstrap_schema_connection, has_any_charged, has_charged_connection, load, save,
    validate, validate_capacity_policy_connection, validate_connection,
};
