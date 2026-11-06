// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "session/authority.rs"]
mod authority;
#[path = "session/delete.rs"]
mod delete;
#[path = "session/open.rs"]
mod open;
#[path = "session/proof.rs"]
mod proof;
#[path = "session/refresh.rs"]
mod refresh;

pub(crate) use authority::{
    ReturnedSessionRefreshTransportError, SessionRefreshStartupAuthority, VerifiedSessionRefresh,
};
pub(crate) use delete::delete_removed_connection_session;
pub(crate) use open::open_connection_session;
pub(crate) use proof::{CompletedHeaderPoll, DeletedSession204, SessionDeleteOutcome};
pub(crate) use refresh::refresh_bound_connection_session;

use crate::error::HostError;
use crate::scale_set::EnsureError;

pub(super) fn map_journal(error: HostError) -> EnsureError {
    match error {
        HostError::Endpoint => EnsureError::Endpoint,
        _ => EnsureError::Unexpected {
            status: 0,
            step: "journal",
        },
    }
}
