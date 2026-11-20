// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::config::ConnectionId;
use crate::error::HostError;
use crate::journal::Journal;
use crate::journal::capacity_header::{
    HeaderAllocation, bootstrap, load, validate, validate_capacity_policy_connection,
};

use crate::journal::connection::storage::read_transaction;

impl Journal {
    pub(crate) async fn bootstrap_header_authority(&self) -> Result<(), HostError> {
        bootstrap(self).await
    }

    pub(crate) async fn validate_header_authority(&self) -> Result<(), HostError> {
        validate(self).await
    }

    pub(crate) async fn header_allocations(&self) -> Result<Vec<HeaderAllocation>, HostError> {
        read_transaction(self, async |connection| {
            let ledger = load(connection).await?;
            Ok(ledger.charged_allocations())
        })
        .await
    }

    pub(crate) async fn header_fairness_cursor(&self) -> Result<Option<ConnectionId>, HostError> {
        read_transaction(self, async |connection| {
            let ledger = load(connection).await?;
            Ok(ledger.fairness_cursor())
        })
        .await
    }
}

pub(crate) async fn validate_header_capacity_policy(
    connection: &turso::Connection,
) -> Result<(), HostError> {
    validate_capacity_policy_connection(connection).await
}
