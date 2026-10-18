//! Streaming verifier and deterministic tag binding for worker image archives.

use super::super::{ProviderFailure, hex, valid_hex, valid_sha};

mod archive_read;
mod derive;
mod layout;
mod model;
mod provenance;
mod runtime;
mod source;
mod verify;

#[cfg(test)]
mod tests;

pub(in crate::launch::resource_probe::provider) use derive::derive_load_archive;
pub(in crate::launch::resource_probe::provider) use model::{
    DerivedLoadArchive, ParsedWorkerArchive, WorkerImage,
};
pub(in crate::launch::resource_probe::provider) use verify::verify;
