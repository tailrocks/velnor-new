// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::{ProviderFailure, hex, valid_hex, valid_sha};

mod archive_read;
mod layout;
mod model;
mod projected;
pub(crate) use projected::{ProjectedCandidate, inspect_projected_candidate};
mod projected_layers;
mod provenance;
mod runtime;
mod source;
pub use model::WorkerImage;
pub use provenance::verify_worker_attestation_manifest;
pub use runtime::verify_worker_runtime_config;
pub use source::verify_worker_build_source;
