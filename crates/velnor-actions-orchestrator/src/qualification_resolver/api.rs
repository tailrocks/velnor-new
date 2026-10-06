//! Pinned read-only GitHub metadata and artifact lookup.

#[path = "api/artifact.rs"]
mod artifact;
#[path = "api/client.rs"]
mod client;
#[path = "api/run.rs"]
mod run;

pub(in crate::qualification_resolver) use artifact::{FetchedNode, resolve_chain};
pub(in crate::qualification_resolver) use client::GitHub;
