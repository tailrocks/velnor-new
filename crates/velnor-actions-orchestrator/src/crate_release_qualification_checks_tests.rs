// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use serde_json::json;
use std::cell::RefCell;

struct Fixture(RefCell<Vec<Value>>);
impl GitHubAdmissionRead for Fixture {
    fn get(&self, _path: &str) -> Result<Value, OrchestratorError> {
        if self.0.borrow().is_empty() {
            return Err(internal("unexpected_page"));
        }
        Ok(self.0.borrow_mut().remove(0))
    }
    fn get_optional(&self, path: &str) -> Result<Option<Value>, OrchestratorError> {
        self.get(path).map(Some)
    }
}
#[test]
fn inventory_requires_complete_distinct_same_head_pages() -> Result<(), Box<dyn std::error::Error>>
{
    let sha = "a".repeat(40);
    let entries: Vec<Value> = (1..=100)
        .map(|id| json!({"id":id,"head_sha":sha}))
        .collect();
    let first = json!({"total_count":101,"check_runs":entries});
    let last = json!({"total_count":101,"check_runs":[{"id":101,"head_sha":sha}]});
    assert_eq!(
        complete_check_runs(
            &Fixture(RefCell::new(vec![first.clone(), last])),
            "owner/repo",
            &sha
        )?
        .len(),
        101
    );
    for bad in [
        json!({"total_count":101,"check_runs":[]}),
        json!({"total_count":101,"check_runs":[{"id":1,"head_sha":sha}]}),
        json!({"total_count":102,"check_runs":[{"id":101,"head_sha":sha}]}),
    ] {
        assert!(
            complete_check_runs(
                &Fixture(RefCell::new(vec![first.clone(), bad])),
                "owner/repo",
                &sha
            )
            .is_err()
        );
    }
    Ok(())
}
