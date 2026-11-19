// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! Source preview helpers: `verify-runtime`, `verify-mac-proof` and `run-authority`.
//!
//! Each helper replaces embedded Python that the old engine renders into
//! `source-preview-runtime.yml`, `source-preview-mac-proof.yml` and
//! `source-preview-publication.yml`. This module holds the pure checks, which run
//! against the [`net::Net`] trait and the [`attest::Attestor`] trait, so tests need no
//! network, `gh` or shell. The real network, `gh` process and file system live in
//! `crate::io::preview`.

pub mod attest;
pub mod authority;
pub mod json;
pub mod net;
pub mod record;
pub mod release;
pub mod runtime;
pub mod sealed;
pub mod stage;

/// The repository every source preview identity is bound to.
pub const REPOSITORY: &str = "tailrocks/velnor-new";
pub const REPOSITORY_ID: u64 = 1_390_620_900;
pub const OWNER: &str = "tailrocks";
pub const OWNER_ID: &str = "78806509";
pub const TARGET: &str = "x86_64-unknown-linux-gnu";
pub const DARWIN_TARGET: &str = "aarch64-apple-darwin";
pub const SIGNER_WORKFLOW: &str = ".github/workflows/product-release-generator.yml";
pub const CI_WORKFLOW: &str = ".github/workflows/ci.yml";
pub const QUALIFICATION_WORKFLOW: &str = ".github/workflows/source-qualification.yml";
/// The only workflow that may sign a preview receipt.
pub const AUTHORITY_WORKFLOW: &str = ".github/workflows/source-preview-publication.yml";
/// The commit of the publication workflow that staged previews are signed by.
pub const PUBLICATION_AUTHORITY_SHA: &str = "72701af69f6f9b693e639814fcf479409da3295c";
pub const MAX_LOCK: usize = 1_048_576;
pub const MAX_JSON: usize = 2_097_152;
pub const MAX_RECEIPT: usize = 65_536;
pub const MAX_BUNDLE: usize = 4_194_304;
pub const MAX_PAYLOAD: usize = 268_435_456;
