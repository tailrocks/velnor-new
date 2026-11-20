// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod completion_intake;
mod positive_poll;
mod scoped;

use super::{ack_ready, steps};

use scoped::poll_scoped_queue;

pub(crate) use positive_poll::{
    CursorDisposition, PositiveAdmission, PositivePollContext, PositivePollOutcome, Settlement,
    admit_when_ready, positive_poll_once_with_pressure,
};
pub(crate) use scoped::{
    ScopedPollContext, ScopedPollOutcome, ZeroHeaderMode, poll_scoped_connection_once,
    poll_scoped_connection_once_with_mode,
};
