// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(crate) mod facts;
mod git;
mod mirror;
pub(crate) mod proto;

use std::path::PathBuf;

pub(crate) use facts::TrustedFacts;
pub(crate) use git::{GIT_ENV, git_args};
pub(crate) use mirror::mirror_path;
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
