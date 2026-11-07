// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use super::BASE_APPROVALS;

pub(super) const STAGED_ENTRY: &str = r#"{
    "repository": "tailrocks/velnor-new",
    "version": "0.1.75",
    "commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "manifest_digest": "sha256:715c8951a8a6bcbb7f17040e67b17350dab014a48ed113d26a96fdd0df8d02c8",
    "binaries": [
      {
        "target": "x86_64-unknown-linux-gnu",
        "artifact": "https://github.com/tailrocks/velnor-new/releases/download/v0.1.75/velnor-actions-0.1.75-x86_64-unknown-linux-gnu",
        "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
      },
      {
        "target": "aarch64-apple-darwin",
        "artifact": "https://github.com/tailrocks/velnor-new/releases/download/v0.1.75/velnor-actions-0.1.75-aarch64-apple-darwin",
        "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
      },
      {
        "target": "x86_64-apple-darwin",
        "artifact": "https://github.com/tailrocks/velnor-new/releases/download/v0.1.75/velnor-actions-0.1.75-x86_64-apple-darwin",
        "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
      }
    ],
    "gh": "2.103.2",
    "mise_bootstrap": {
      "version": "2026.11.1",
      "artifact": "https://github.com/jdx/mise/releases/download/v2026.11.1/mise-v2026.11.1-linux-x64",
      "sha256": "7129dbab31d698f9028db9374f7ba7987232b0426f837dcbce437130a61415f9"
    },
    "mise_runtime": [
      {"target": "x86_64-unknown-linux-gnu", "version": "2026.10.20", "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "bytes": 173449633},
      {"target": "aarch64-apple-darwin", "version": "2026.10.20", "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "bytes": 138238817},
      {"target": "x86_64-apple-darwin", "version": "2026.10.20", "sha256": "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc", "bytes": 169788225}
    ],
    "state": "staged"
  }"#;

pub(super) fn staged_fixture_approvals() -> Result<String, Box<dyn Error>> {
    let mut approvals: serde_json::Value = serde_json::from_str(BASE_APPROVALS)?;
    approvals["staged"] = serde_json::from_str(STAGED_ENTRY)?;
    Ok(serde_json::to_string_pretty(&approvals)?)
}
