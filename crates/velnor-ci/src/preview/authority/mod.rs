// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! `run-authority`: the draft, prepare and publish steps of
//! `source-preview-publication.yml`, which take a preview from the admitted source
//! metadata to an immutable, attested GitHub release.

pub mod files;
pub mod github;
pub mod model;
pub mod modes;
pub mod pair;
pub mod zip;

use std::fmt::Debug;

use serde_json::Value;

use super::json::Res;
use super::net::Net;

/// GitHub as the publication steps use it: reads through [`Net`], and the writes and
/// artifact downloads below. The real client holds the token, sends it only to
/// `api.github.com` and `uploads.github.com`, and never to an artifact blob host.
pub trait Client: Net + Debug {
    /// GET `/repos/tailrocks/velnor-new` + `suffix`.
    fn get(&self, suffix: &str) -> Res<Value> {
        self.api(&super::net::repo_path(suffix))
    }
    /// POST or PATCH `suffix` with a JSON body; the status must be one of `allowed`.
    fn send(&self, method: &str, suffix: &str, body: &[u8], allowed: &[u16]) -> Res<Value>;
    /// POST an asset to `uploads.github.com` for the release.
    fn upload(&self, release_id: u64, name: &str, data: &[u8]) -> Res<Value>;
    /// Download the zip of an Actions artifact of the given size and digest.
    fn artifact_zip(&self, id: u64, size: u64, digest: &str) -> Res<Vec<u8>>;
}
