// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

pub const SENDER: &str = "velnor-local";
pub const NO_COMMIT: &str = "0000000000000000000000000000000000000000";

#[must_use]
pub fn for_standin(mut payload: Value) -> Value {
    if payload["workflow"].is_string()
        && let Some(object) = payload.as_object_mut()
    {
        object.remove("workflow");
    }
    payload
}

#[derive(Debug)]
pub struct Repository<'a> {
    pub full_name: &'a str,
    pub default_branch: &'a str,
}

#[derive(Debug)]
pub struct Push<'a> {
    pub branch: &'a str,
    pub before: &'a str,
    pub after: &'a str,
}

#[derive(Debug)]
pub struct PullRequest<'a> {
    pub action: &'a str,
    pub number: u64,
    pub base: (&'a str, &'a str),
    pub head: (&'a str, &'a str),
    pub before: Option<&'a str>,
    pub merged: bool,
    pub changed_files: usize,
}

#[derive(Debug)]
pub struct MergeGroup<'a> {
    pub base: (&'a str, &'a str),
    pub head: (&'a str, &'a str),
}

impl Repository<'_> {
    fn owner(&self) -> &str {
        self.full_name.split('/').next().unwrap_or_default()
    }

    fn json(&self) -> Value {
        json!({
            "full_name": self.full_name,
            "name": self.full_name.split('/').nth(1).unwrap_or_default(),
            "owner": {"login": self.owner()},
            "default_branch": self.default_branch,
            "fork": false,
        })
    }

    fn common(&self) -> Value {
        json!({
            "repository": self.json(),
            "organization": {"login": self.owner()},
            "sender": {"login": SENDER},
        })
    }

    #[must_use]
    pub fn push(&self, push: &Push<'_>) -> Value {
        let mut payload = self.common();
        payload["ref"] = json!(format!("refs/heads/{}", push.branch));
        payload["before"] = json!(push.before);
        payload["after"] = json!(push.after);
        payload["base_ref"] = Value::Null;
        payload["created"] = json!(push.before == NO_COMMIT);
        payload["deleted"] = json!(false);
        payload["forced"] = json!(false);
        payload["compare"] = json!(format!("{}...{}", push.before, push.after));
        payload["pusher"] = json!({"name": SENDER});
        payload["head_commit"] = json!({"id": push.after});
        payload["commits"] = json!([{"id": push.after}]);
        payload
    }

    #[must_use]
    pub fn pull_request(&self, pull: &PullRequest<'_>) -> Value {
        let side =
            |(name, sha): (&str, &str)| json!({"ref": name, "sha": sha, "repo": self.json()});
        let mut payload = self.common();
        payload["action"] = json!(pull.action);
        payload["number"] = json!(pull.number);
        if let Some(before) = pull.before {
            payload["before"] = json!(before);
            payload["after"] = json!(pull.head.1);
        }
        payload["pull_request"] = json!({
            "number": pull.number,
            "state": if pull.action == "closed" { "closed" } else { "open" },
            "title": format!("{} into {}", pull.head.0, pull.base.0),
            "draft": false,
            "merged": pull.merged,
            "labels": [],
            "changed_files": pull.changed_files,
            "user": {"login": SENDER},
            "base": side(pull.base),
            "head": side(pull.head),
        });
        payload
    }

    #[must_use]
    pub fn schedule(&self, cron: &str, workflow: &str) -> Value {
        json!({
            "schedule": cron,
            "workflow": workflow,
            "repository": self.json(),
            "organization": {"login": self.owner()},
        })
    }

    #[must_use]
    pub fn workflow_dispatch(&self, branch: &str, inputs: &Value, workflow: &str) -> Value {
        let mut payload = self.common();
        payload["ref"] = json!(format!("refs/heads/{branch}"));
        payload["inputs"] = inputs.clone();
        payload["workflow"] = json!(workflow);
        payload
    }

    #[must_use]
    pub fn merge_group(&self, group: &MergeGroup<'_>) -> Value {
        let mut payload = self.common();
        payload["action"] = json!("checks_requested");
        payload["merge_group"] = json!({
            "head_sha": group.head.1,
            "head_ref": format!("refs/heads/{}", group.head.0),
            "base_sha": group.base.1,
            "base_ref": format!("refs/heads/{}", group.base.0),
            "head_commit": {"id": group.head.1},
        });
        payload
    }
}
