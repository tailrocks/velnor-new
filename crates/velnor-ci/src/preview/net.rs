// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::time::Instant;

use serde_json::Value;

use super::json::Res;

/// Read access to GitHub for the source preview checks.
///
/// The real implementation (`crate::io::preview::Transport`) allows only
/// `https://api.github.com/repos/tailrocks/velnor-new/...` for the API and the release
/// hosts for assets, never sends the token to an asset host, and gives up at the deadline.
pub trait Net {
    /// GET `path`, which starts with `/repos/tailrocks/velnor-new/`, and parse the JSON.
    fn api(&self, path: &str) -> Res<Value>;
    /// GET a release asset of the repository without credentials, at most `maximum` bytes.
    fn asset(&self, url: &str, maximum: usize) -> Res<Vec<u8>>;
    /// The time after which every request fails.
    fn deadline(&self) -> Instant;
}

#[must_use]
pub fn repo_path(suffix: &str) -> String {
    format!("/repos/{}{suffix}", super::REPOSITORY)
}
