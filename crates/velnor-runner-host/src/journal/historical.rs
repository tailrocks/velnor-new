// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::schema;
use crate::error::HostError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Historical {
    V45,
    V46,
    V47,
    V48,
}

impl Historical {
    pub(super) const fn key(self) -> &'static str {
        match self {
            Self::V45 => "v45",
            Self::V46 => "v46",
            Self::V47 => "v47",
            Self::V48 => "v48",
        }
    }

    pub(super) async fn bootstrap(self, conn: &turso::Connection) -> Result<(), HostError> {
        match self {
            Self::V45 => schema::bootstrap_historical_v45(conn).await,
            Self::V46 => schema::bootstrap_historical_v46(conn).await,
            Self::V47 => schema::bootstrap_historical_v47(conn).await,
            Self::V48 => schema::bootstrap_historical_v48(conn).await,
        }
    }
}
