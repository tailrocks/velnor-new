// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};

pub const SENDER: &str = "velnor-local";
pub const NO_COMMIT: &str = "0000000000000000000000000000000000000000";

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
    pub number: u64,
    pub base: (&'a str, &'a str),
    pub head: (&'a str, &'a str),
    pub changed_files: usize,
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
        payload["action"] = json!("opened");
        payload["number"] = json!(pull.number);
        payload["pull_request"] = json!({
            "number": pull.number,
            "state": "open",
            "title": format!("{} into {}", pull.head.0, pull.base.0),
            "draft": false,
            "merged": false,
            "labels": [],
            "changed_files": pull.changed_files,
            "user": {"login": SENDER},
            "base": side(pull.base),
            "head": side(pull.head),
        });
        payload
    }
}
