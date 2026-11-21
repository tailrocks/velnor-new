// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::api::checking;
use super::{Repo, read};
use serde_json::json;
use std::collections::BTreeMap;
use std::process::Command;

use velnor_ci::report::SaveOutcome;

#[test]
fn target_and_sources_saves_under_a_new_key_are_looked_up_under_the_saved_key() -> super::ResultTest
{
    for (family, step) in [("target", "target_key"), ("sources", "sources_rust_key")] {
        for (recorded, expected) in [
            (Some("new-key"), SaveOutcome::Saved),
            (Some(""), SaveOutcome::Failed),
            (None, SaveOutcome::Failed),
        ] {
            let repo = Repo::new("report-saved-key-target-sources");
            let plan = json!([{"family":family,"root":null,"restore_step":"restore","save_step":"save","save_key_step":step}]);
            let mut steps = json!({
                "restore":{"outcome":"success","outputs":{"cache-primary-key":"old-key","cache-hit":"false"}},
                "save":{"outcome":"success"},
                step: {"outcome":"success","outputs":{}}
            });
            if let Some(key) = recorded {
                steps[step]["outputs"]["key"] = json!(key);
            }
            let listing = |key: &str| {
                let entries = if key == "new-key" {
                    json!([{"key":key,"ref":"refs/heads/main","size_in_bytes":12}])
                } else {
                    json!([])
                };
                json!({"total_count":entries.as_array().map_or(0, Vec::len),"actions_caches":entries}).to_string()
            };
            let api = crate::io::serve(BTreeMap::from([
                (
                    "/repos/owner/repo/actions/runs/1/attempts/1/jobs?per_page=100&page=1"
                        .to_owned(),
                    json!({"total_count":0,"jobs":[]}).to_string(),
                ),
                (
                    "/repos/owner/repo/actions/caches?key=old-key&per_page=100".to_owned(),
                    listing("old-key"),
                ),
                (
                    "/repos/owner/repo/actions/caches?key=new-key&per_page=100".to_owned(),
                    listing("new-key"),
                ),
            ]));
            let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
                .arg("report")
                .current_dir(repo.root())
                .env("RUNNER_TEMP", repo.root())
                .env("GITHUB_JOB", "rust")
                .env("GITHUB_REF", "refs/heads/main")
                .env("VELNOR_REPORT_DEFAULT_BRANCH", "main")
                .env("GITHUB_REPOSITORY", "owner/repo")
                .env("GITHUB_RUN_ID", "1")
                .env("GITHUB_RUN_ATTEMPT", "1")
                .env("VELNOR_REPORT_CACHES", plan.to_string())
                .env("VELNOR_REPORT_STEPS", steps.to_string())
                .output()?;
            assert!(output.status.success(), "{output:?}");
            let checked = checking(&repo)
                .env("GITHUB_TOKEN", "fixture")
                .env("GITHUB_API_URL", api)
                .output()?;
            assert_eq!(
                checked.status.success(),
                expected == SaveOutcome::Saved,
                "{family} {recorded:?}"
            );
            let metrics = read(&repo);
            assert_eq!(metrics.caches[0].requested_key, "old-key", "{family}");
            assert_eq!(metrics.caches[0].save, expected, "{family} {recorded:?}");
            assert_eq!(
                metrics.violations.is_empty(),
                expected == SaveOutcome::Saved,
                "{family} {recorded:?}"
            );
        }
    }
    Ok(())
}
