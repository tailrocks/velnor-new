// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "compiler_observation/report.rs"]
mod report;
#[path = "compiler_observation/session.rs"]
mod session;
#[path = "compiler_observation/validation.rs"]
mod validation;

pub(crate) use session::{ChildOutcome, ObservationReport, ObservationSession, new_session_id};
