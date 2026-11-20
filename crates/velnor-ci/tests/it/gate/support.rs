// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde_json::{Value, json};
use velnor_ci::gate::{Verdict, release_gate};
use velnor_ci::github::{branch_path, rules_path, run_jobs_path, runs_path};

use super::{GATE, GREEN, GREEN_CI, VERDICT};
use crate::support::repo;

pub(super) struct Recorded(pub(super) BTreeMap<String, Value>);

impl Recorded {
    pub(super) fn gate_with_budget(&self, limit: usize) -> Result<Verdict, String> {
        struct Budget<'a> {
            responses: &'a BTreeMap<String, Value>,
            left: std::cell::Cell<usize>,
        }
        impl velnor_ci::github::GitHub for Budget<'_> {
            fn get(&self, path: &str) -> Result<String, velnor_ci::error::CiError> {
                let left =
                    self.left.get().checked_sub(1).ok_or_else(|| {
                        velnor_ci::error::CiError::new("request budget exhausted")
                    })?;
                self.left.set(left);
                self.responses
                    .get(path)
                    .map(Value::to_string)
                    .ok_or_else(|| {
                        velnor_ci::error::CiError::new(format!("no recorded response for {path}"))
                    })
            }
        }
        release_gate(
            &Budget {
                responses: &self.0,
                left: std::cell::Cell::new(limit),
            },
            &repo(),
            GREEN,
            &["DCO".to_owned()],
            None,
        )
        .map_err(|e| e.to_string())
    }
    pub(super) fn new() -> Self {
        Self(serde_json::from_str(GATE).unwrap_or_else(|e| unreachable!("{e}")))
    }

    pub(super) fn set(mut self, path: String, body: Value) -> Self {
        self.0.insert(path, body);
        self
    }

    pub(super) fn edit(mut self, path: &str, change: impl FnOnce(&mut Value)) -> Self {
        change(self.0.get_mut(path).unwrap_or_else(|| unreachable!()));
        self
    }

    pub(super) fn rules(self, rules: Vec<Value>) -> Self {
        self.set(rules_path(&repo(), "main", 1), Value::Array(rules))
    }

    pub(super) fn requiring(self, checks: &[Value]) -> Self {
        self.rules(vec![json!({"type": "deletion"}), required(checks)])
    }

    pub(super) fn green_run(self, change: impl Fn(&mut Value)) -> Self {
        self.edit(&runs_path(&repo(), GREEN, 1), |runs| {
            for run in each(&mut runs["workflow_runs"]) {
                if run["id"] == json!(GREEN_CI) {
                    change(run);
                }
            }
        })
    }

    pub(super) fn green_job(self, name: &str, change: impl Fn(&mut Value)) -> Self {
        self.edit(&run_jobs_path(&repo(), GREEN_CI, 1), |jobs| {
            for job in each(&mut jobs["jobs"]) {
                if job["name"] == json!(name) {
                    change(job);
                }
            }
        })
    }

    pub(super) fn protection(
        self,
        branch: &str,
        level: &str,
        contexts: &[&str],
        checks: &Value,
    ) -> Self {
        self.set(
            branch_path(&repo(), branch),
            json!({"protection": {"enabled": level != "off", "required_status_checks": {"enforcement_level": level, "contexts": contexts, "checks": checks}}}),
        )
    }

    pub(super) fn total(self, path: &str, total: usize) -> Self {
        self.edit(path, |page| page["total_count"] = json!(total))
    }

    pub(super) fn gate(&self, sha: &str) -> Result<Verdict, String> {
        self.gate_with(sha, &[])
    }

    pub(super) fn gate_with(&self, sha: &str, pr_only: &[String]) -> Result<Verdict, String> {
        self.gate_in_run(sha, pr_only, None)
    }
}

pub(super) fn each(list: &mut Value) -> impl Iterator<Item = &mut Value> {
    list.as_array_mut()
        .unwrap_or_else(|| unreachable!())
        .iter_mut()
}

pub(super) fn check(context: &str) -> Value {
    json!({"context": context})
}

pub(super) fn required(checks: &[Value]) -> Value {
    json!({"type": "required_status_checks", "parameters": {"strict_required_status_checks_policy": true, "do_not_enforce_on_create": false, "required_status_checks": checks}, "ruleset_source_type": "Repository", "ruleset_source": "tailrocks/velnor-new", "ruleset_id": 24_396_608})
}

pub(super) fn assert_refused(said: &Result<Verdict, String>, part: &str) {
    assert!(said.as_ref().is_err_and(|e| e.contains(part)), "{said:?}");
}

pub(super) fn both() -> Recorded {
    Recorded::new().requiring(&[check("Required"), check(VERDICT)])
}

pub(super) fn verdict(ready: bool, sha: &str, checks: &str) -> Verdict {
    Verdict {
        ready,
        report: format!("commit {sha} is on main\n{checks}ready={ready}\n"),
    }
}

pub(super) fn concluded(sha: &str, check: &str, conclusion: &str) -> Result<Verdict, String> {
    Err(format!(
        "check {check} did not pass on commit {sha}: it concluded {conclusion}"
    ))
}

pub(super) fn absent(check: &str) -> String {
    format!("check {check} did not pass on commit {GREEN}: no job of a push run on main reports it")
}

pub(super) fn assert_not_counted(case: &str, change: impl Fn(&mut Value)) {
    let said = both().green_run(change).gate(GREEN);
    let both_absent = format!("{}\n{}", absent("Required"), absent(VERDICT));
    assert_eq!(said, Err(both_absent), "{case}");
}

pub(super) fn failing(job: &mut Value) {
    job["conclusion"] = json!("failure");
}
