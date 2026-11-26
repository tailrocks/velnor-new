// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The store broker: the only code that writes a store during a job. Adapters
//! send it a [`Request`]; it decides the repository, the path and the commit
//! from the host's [`TrustedFacts`]. Here it runs in the runner's process;
//! the sandbox parent serves the same requests over a socket (#2911).

pub(crate) mod facts;
mod mirror;
pub(crate) mod proto;

use std::path::PathBuf;

pub(crate) use facts::TrustedFacts;
pub(crate) use mirror::{GIT_ENV, git_args, mirror_path};
pub(crate) use proto::{Request, Response, Token};

pub(crate) struct Broker {
    facts: Box<TrustedFacts>,
    root: PathBuf,
}

impl Broker {
    pub(crate) fn new(facts: TrustedFacts, root: PathBuf) -> Self {
        Self {
            facts: Box::new(facts),
            root,
        }
    }

    pub(crate) async fn handle(&self, request: Request) -> Response {
        match request {
            Request::FetchMirror { token } => {
                let token = token.as_ref().map(Token::expose);
                match mirror::fetch(&self.root, &self.facts, token).await {
                    Ok(()) => Response::Done,
                    Err(error) => Response::Failed {
                        message: format!("{error:#}"),
                    },
                }
            }
        }
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
