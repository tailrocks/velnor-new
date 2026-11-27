// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(crate) mod facts;
mod git;
mod location;
mod mirror;
pub(crate) mod proto;
mod repair;
mod transfer;
mod walk;

use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, anyhow};

use mirror::Repair;

pub(crate) use facts::TrustedFacts;
pub(crate) use git::{TREE_PROBE, git_args, isolate};
pub(crate) use location::mirror_path;
pub(crate) use proto::{FailedStep, Request, Response, Token, Window};

const REPAIR_CAP: Duration = Duration::from_mins(10);
const FETCH_CAP: Duration = Duration::from_mins(30);

async fn within<T>(what: &str, cap: Duration, work: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(cap, work)
        .await
        .unwrap_or_else(|_| Err(anyhow!("{what} took longer than {} seconds", cap.as_secs())))
}

pub(crate) struct Broker {
    facts: Box<TrustedFacts>,
    root: PathBuf,
    repair_cap: Duration,
    fetch_cap: Duration,
}

impl Broker {
    pub(crate) fn new(facts: TrustedFacts, root: PathBuf) -> Self {
        Self {
            facts: Box::new(facts),
            root,
            repair_cap: REPAIR_CAP,
            fetch_cap: FETCH_CAP,
        }
    }

    #[cfg(test)]
    fn with_repair_cap(mut self, cap: Duration) -> Self {
        self.repair_cap = cap;
        self
    }

    #[cfg(test)]
    fn with_fetch_cap(mut self, cap: Duration) -> Self {
        self.fetch_cap = cap;
        self
    }

    pub(crate) async fn handle(&self, request: Request) -> Response {
        let result = match request {
            Request::FetchMirror { token } => {
                let token = token.as_ref().map(Token::expose);
                mirror::fetch(&self.root, &self.facts, token, self.fetch_cap)
                    .await
                    .map(|notices| Response::Done { notices })
            }
            Request::RepairMirror {
                token,
                window,
                failed,
            } => {
                let token = token.as_ref().map(Token::expose);
                let cap = self.repair_cap;
                mirror::repair(&self.root, &self.facts, token, window, failed, cap)
                    .await
                    .map(|repair| match repair {
                        Repair::Sound => Response::Sound,
                        Repair::Repaired(notices) => Response::Done { notices },
                    })
            }
        };
        result.unwrap_or_else(|error| Response::Failed {
            message: format!("{error:#}"),
        })
    }
}

pub(crate) enum StoreWriter {
    InProcess(Broker),
    Unavailable(String),
}

impl StoreWriter {
    pub(crate) fn facts(&self) -> Result<&TrustedFacts, String> {
        match self {
            Self::InProcess(broker) => Ok(&broker.facts),
            Self::Unavailable(reason) => Err(reason.clone()),
        }
    }

    pub(crate) async fn request(&self, request: Request) -> Response {
        match self {
            Self::InProcess(broker) => broker.handle(request).await,
            Self::Unavailable(reason) => Response::Failed {
                message: reason.clone(),
            },
        }
    }
}

#[cfg(test)]
mod tests;
