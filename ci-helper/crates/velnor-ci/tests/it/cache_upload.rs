// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::io::{Repo, serve};
use std::collections::BTreeMap;
use std::process::Command;

#[test]
fn a_cache_upload_must_have_an_exact_entry_before_it_can_report_success() {
    for key in ["base-1", "base-other", ""] {
        let entries = if key.is_empty() {
            "[]".to_owned()
        } else {
            format!("[{{\"id\":1,\"key\":{key:?},\"created_at\":\"2026-11-17T00:00:00Z\"}}]")
        };
        let api = serve(BTreeMap::from([(
            "/repos/owner/name/actions/caches?key=base-1&ref=refs%2Fheads%2Fmain&per_page=100"
                .to_owned(),
            format!("{{\"actions_caches\":{entries}}}"),
        )]));
        let repo = Repo::new("cache-upload");
        let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["cache", "verify", "--key", "base-1"])
            .env("GITHUB_TOKEN", "test-token")
            .env("GITHUB_REPOSITORY", "owner/name")
            .env("GITHUB_REF", "refs/heads/main")
            .env("GITHUB_API_URL", api)
            .current_dir(repo.root())
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert_eq!(output.status.success(), key == "base-1", "{output:?}");
        if key != "base-1" {
            assert!(
                String::from_utf8_lossy(&output.stderr).contains("was not saved"),
                "{output:?}"
            );
        }
    }
}
