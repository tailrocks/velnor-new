// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod access;
mod budget;
pub(crate) mod cache;
pub(crate) mod facts;
mod git;
mod incoming;
mod layout;
mod live;
mod location;
mod mirror;
pub(crate) mod proto;
mod publish;
mod repair;
pub(crate) mod safe_fs;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_abandon;
#[cfg(test)]
mod tests_budget;
#[cfg(test)]
mod tests_commit;
#[cfg(test)]
mod tests_enter;
#[cfg(test)]
mod tests_entry;
#[cfg(test)]
mod tests_events;
#[cfg(test)]
mod tests_gc;
#[cfg(test)]
mod tests_live;
#[cfg(test)]
mod tests_purge;
#[cfg(test)]
mod tests_repair;
#[cfg(test)]
mod tests_swap;
#[cfg(test)]
mod tests_upload;
mod transfer;
mod trusted;
mod upload;
mod walk;

use std::future::Future;
use std::io::PipeWriter;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Result, anyhow};

use mirror::Repair;

pub(crate) use facts::TrustedFacts;
pub(crate) use git::{TREE_PROBE, git_args, isolate};
use layout::Layout;
pub(crate) use location::mirror_path;
pub(crate) use proto::{FailedStep, Request, Response, Token, Window};
use upload::Session;

const REPAIR_CAP: Duration = Duration::from_mins(10);
const FETCH_CAP: Duration = Duration::from_mins(30);

async fn within<T>(what: &str, cap: Duration, work: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::time::timeout(cap, work)
        .await
        .unwrap_or_else(|_| Err(anyhow!("{what} took longer than {} seconds", cap.as_secs())))
}

pub(crate) struct Reply {
    pub(crate) response: Response,
    pub(crate) pipe: Option<PipeWriter>,
}

impl From<Response> for Reply {
    fn from(response: Response) -> Self {
        Self {
            response,
            pipe: None,
        }
    }
}

fn mirrored(result: anyhow::Result<Response>) -> Reply {
    result
        .unwrap_or_else(|error| Response::Failed {
            message: format!("{error:#}"),
        })
        .into()
}

fn failed(message: &str) -> Reply {
    Response::Failed {
        message: message.to_owned(),
    }
    .into()
}

pub(crate) struct Broker {
    facts: Box<TrustedFacts>,
    layout: Layout,
    repair_cap: Duration,
    fetch_cap: Duration,
}

impl Broker {
    pub(crate) fn new(facts: TrustedFacts, root: PathBuf) -> Self {
        Self {
            facts: Box::new(facts),
            layout: Layout::new(root),
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

    #[cfg(test)]
    fn with_lock_wait(mut self, wait: std::time::Duration) -> Self {
        self.layout.lock_wait = wait;
        self
    }

    #[cfg(test)]
    fn with_budget(mut self, budget: u64) -> Self {
        self.layout.budget = budget;
        self
    }

    #[cfg(test)]
    fn with_clock(mut self, clock: layout::Clock) -> Self {
        self.layout.clock = clock;
        self
    }

    #[cfg(test)]
    fn with_max_data(mut self, limit: u64) -> Self {
        self.layout.max_data = limit;
        self
    }

    pub(crate) async fn begin_cache(
        &self,
        key: &str,
        version: &str,
    ) -> Result<CacheUpload<'_>, Response> {
        let mut slot = None;
        let request = Request::BeginCache {
            key: key.to_owned(),
            version: version.to_owned(),
        };
        match self.handle(&mut slot, request).await {
            Reply {
                response: Response::Ready,
                pipe: Some(writer),
            } => Ok(CacheUpload {
                broker: self,
                slot,
                writer: Some(writer),
            }),
            Reply { response, .. } => Err(response),
        }
    }

    pub(crate) async fn handle(&self, slot: &mut Option<Session>, request: Request) -> Reply {
        match request {
            Request::FetchMirror { token } => {
                let token = token.as_ref().map(Token::expose);
                mirrored(
                    mirror::fetch(&self.layout.root, &self.facts, token, self.fetch_cap)
                        .await
                        .map(|notices| Response::Done { notices }),
                )
            }
            Request::VerifyPin { token, pin, path } => {
                let token = token.as_ref().map(Token::expose);
                mirror::verify_pin(&self.root, &self.facts, token, &pin, &path)
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
                mirrored(
                    mirror::repair(&self.layout.root, &self.facts, token, window, failed, cap)
                        .await
                        .map(|repair| match repair {
                            Repair::Sound => Response::Sound,
                            Repair::Repaired(notices) => Response::Done { notices },
                        }),
                )
            }
            Request::BeginCache { key, version } => {
                if slot.is_some() {
                    return failed("a cache upload is already open on this connection");
                }
                match upload::begin(&self.layout, &self.facts, key, version) {
                    Ok((session, pipe)) => {
                        *slot = Some(session);
                        Reply {
                            response: Response::Ready,
                            pipe: Some(pipe),
                        }
                    }
                    Err(response) => response.into(),
                }
            }
            Request::SealCache { bytes } => match slot.take() {
                Some(session) => session.seal(bytes).await.into(),
                None => failed("no cache upload is open on this connection"),
            },
            Request::TouchCache {
                scope,
                key,
                version,
            } => mirrored(
                access::touch(&self.layout, &self.facts, &scope, &key, &version).map(|()| {
                    Response::Done {
                        notices: Vec::new(),
                    }
                }),
            ),
            Request::AbortCache => match slot.take() {
                Some(session) => session.abort().await.into(),
                None => Response::Done {
                    notices: Vec::new(),
                }
                .into(),
            },
        }
    }
}

impl Drop for Broker {
    fn drop(&mut self) {
        self.layout.remove_all();
    }
}

pub(crate) struct CacheUpload<'a> {
    broker: &'a Broker,
    slot: Option<Session>,
    writer: Option<PipeWriter>,
}

impl CacheUpload<'_> {
    pub(crate) fn take_writer(&mut self) -> Option<PipeWriter> {
        self.writer.take()
    }

    pub(crate) async fn seal(mut self, bytes: u64) -> Response {
        self.broker
            .handle(&mut self.slot, Request::SealCache { bytes })
            .await
            .response
    }

    pub(crate) async fn abort(mut self) -> Response {
        self.broker
            .handle(&mut self.slot, Request::AbortCache)
            .await
            .response
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
            Self::InProcess(broker) => broker.handle(&mut None, request).await.response,
            Self::Unavailable(reason) => Response::Failed {
                message: reason.clone(),
            },
        }
    }

    pub(crate) async fn begin_cache(
        &self,
        key: &str,
        version: &str,
    ) -> Result<CacheUpload<'_>, Response> {
        match self {
            Self::InProcess(broker) => broker.begin_cache(key, version).await,
            Self::Unavailable(reason) => Err(Response::Failed {
                message: reason.clone(),
            }),
        }
    }
}
