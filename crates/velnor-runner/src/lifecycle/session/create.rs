// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{SESSION_CONFLICT_ATTEMPTS, Session, api_status, is_retriable, refused, session_id};
use crate::protocol::{BrokerErrorCategory, broker_error_category};

impl Session {
    pub(super) async fn establish(&self) -> anyhow::Result<()> {
        let mut conflicts = 0_u32;
        let mut signed_in = false;
        loop {
            let broker = self.auth().broker.clone();
            let mut failure = match broker.create_session(&self.template).await {
                Ok(created) => {
                    self.auth().id = session_id(created)?;
                    return Ok(());
                }
                Err(error) => error,
            };
            if api_status(&failure) == Some(401) {
                if signed_in {
                    return Err(refused(failure));
                }
                match self.refresh().await {
                    Ok(()) => {
                        signed_in = true;
                        continue;
                    }
                    Err(error) => failure = error,
                }
            }
            signed_in = false;
            let retry = match broker_error_category(&failure) {
                Some(BrokerErrorCategory::Conflict) => {
                    conflicts += 1;
                    conflicts < SESSION_CONFLICT_ATTEMPTS
                }
                _ => is_retriable(&failure),
            };
            if !retry {
                return Err(failure);
            }
            tokio::time::sleep(self.retry_interval).await;
        }
    }
}
