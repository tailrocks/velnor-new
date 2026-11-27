// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    CLOCK_SKEW_ATTEMPTS, SESSION_CONFLICT_ATTEMPTS, Session, api_status, is_retriable, refused,
    session_id,
};
use crate::protocol::{
    BrokerErrorCategory, OAuthErrorKind, broker_error_category, oauth_error_kind,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Token {
    Missing,
    Held,
    Rejected,
    Fresh,
}

impl Token {
    fn after_sign_in(self) -> Option<Self> {
        match self {
            Self::Missing => Some(Self::Held),
            Self::Rejected => Some(Self::Fresh),
            Self::Held | Self::Fresh => None,
        }
    }
}

#[derive(Default)]
struct Failures {
    conflicts: u32,
    clock_skews: u32,
}

impl Failures {
    fn allow_retry(&mut self, error: &anyhow::Error) -> bool {
        match oauth_error_kind(error) {
            Some(OAuthErrorKind::RegistrationDeleted | OAuthErrorKind::Refused) => false,
            Some(OAuthErrorKind::ClockSkew) => {
                self.clock_skews += 1;
                self.clock_skews < CLOCK_SKEW_ATTEMPTS
            }
            Some(OAuthErrorKind::Other) | None => match broker_error_category(error) {
                Some(BrokerErrorCategory::Conflict) => {
                    self.conflicts += 1;
                    self.conflicts < SESSION_CONFLICT_ATTEMPTS
                }
                _ => is_retriable(error),
            },
        }
    }
}

impl Session {
    pub(super) async fn establish(&self, mut token: Token) -> anyhow::Result<()> {
        let mut failures = Failures::default();
        loop {
            if let Some(signed_in) = token.after_sign_in() {
                match self.refresh().await {
                    Ok(()) => token = signed_in,
                    Err(error) => {
                        self.wait_to_retry(&mut failures, error).await?;
                        continue;
                    }
                }
            }
            let broker = self.auth().broker.clone();
            let error = match broker.create_session(&self.template).await {
                Ok(created) => {
                    self.auth().id = session_id(created)?;
                    return Ok(());
                }
                Err(error) => error,
            };
            if api_status(&error) == Some(401) {
                if token == Token::Fresh {
                    return Err(refused(error));
                }
                token = Token::Rejected;
                continue;
            }
            token = Token::Held;
            self.wait_to_retry(&mut failures, error).await?;
        }
    }

    async fn wait_to_retry(
        &self,
        failures: &mut Failures,
        error: anyhow::Error,
    ) -> anyhow::Result<()> {
        if !failures.allow_retry(&error) {
            return Err(error);
        }
        eprintln!("warning: creating the broker session failed, retrying: {error:#}");
        tokio::time::sleep(self.retry_interval).await;
        Ok(())
    }
}
