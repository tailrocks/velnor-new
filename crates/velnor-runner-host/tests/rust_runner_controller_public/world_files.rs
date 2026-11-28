// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{Value, json};

use super::checks::jit_name;
use super::child::{DETACH, DETACH_MS};
use super::events::wait_for;
use super::fixtures::{Spec, TestResult, hex_sha256};
use super::harness::{CHILD_MARKER, World};

pub(super) const CENSUS: &str = "rust-runner-census.json";
pub(super) const TOKEN_A: &str = "0123456789abcdef0123456789abcdef";
pub(super) const TOKEN_B: &str = "fedcba9876543210fedcba9876543210";

pub(super) fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(0))
}

impl World {
    pub(super) fn write_config(&self, default_branch: &str, extra: &[Spec<'_>]) -> TestResult<()> {
        let extra: Vec<Value> = extra
            .iter()
            .map(|spec| {
                json!({
                    "repo": spec.repo,
                    "repository_id": spec.repository_id,
                    "scale_set": spec.scale_set,
                    "labels": ["velnor", spec.scale_set],
                })
            })
            .collect();
        let config = json!({
            "owner": "tailrocks",
            "repo": "velnor-lab",
            "repository_id": 42,
            "scale_set": "velnor-mac",
            "labels": ["velnor", "velnor-mac"],
            "canary_tag": self.tag,
            "queue_url": "https://queue.example.com/messages",
            "admin_url": "https://pipelines.example.com/",
            "store": self.store,
            "ps_table": self.root.join("ps.table"),
            "default_branch": default_branch,
            "extra": extra,
            "state": self.state,
        });
        std::fs::write(self.github.join("config.json"), config.to_string())?;
        Ok(())
    }

    pub(super) fn queue_message_for(
        &self,
        repo: &str,
        id: u64,
        offers: &[Value],
    ) -> TestResult<()> {
        let directory = self.github.join(format!("messages-{repo}"));
        std::fs::create_dir_all(&directory)?;
        let staging = directory.join(format!("{id}.tmp"));
        std::fs::write(&staging, json!({"id": id, "offers": offers}).to_string())?;
        std::fs::rename(staging, directory.join(format!("{id}.json")))?;
        Ok(())
    }

    fn mutate_fake(&self, change: impl FnOnce(&mut Value) -> TestResult<()>) -> TestResult<()> {
        let lock = self.github.join(".lock");
        while std::fs::create_dir(&lock).is_err() {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let outcome = (|| -> TestResult<()> {
            let path = self.github.join("state.json");
            let mut state: Value = serde_json::from_slice(&std::fs::read(&path)?)?;
            change(&mut state)?;
            std::fs::write(&path, state.to_string())?;
            Ok(())
        })();
        std::fs::remove_dir(&lock)?;
        outcome
    }

    pub(super) fn plant_runner(&self, name: &str, scale_set_id: u64) -> TestResult<()> {
        self.mutate_fake(|state| {
            let id = 900 + state["runners"].as_array().map_or(0, Vec::len);
            state["runners"]
                .as_array_mut()
                .ok_or("runners")?
                .push(json!({"id": id, "name": name, "runnerScaleSetId": scale_set_id}));
            Ok(())
        })
    }

    pub(super) fn script_phase(&self, script: &Value) -> TestResult<()> {
        self.script_refusals(script)?;
        self.mutate_fake(|state| {
            state["attempts"] = json!({});
            Ok(())
        })
    }

    pub(super) fn fake_state(&self) -> TestResult<Value> {
        Ok(serde_json::from_slice(&std::fs::read(
            self.github.join("state.json"),
        )?)?)
    }

    pub(super) fn detach(&self, test: &str, id: &str, toml: &str) -> TestResult<String> {
        self.detach_within(test, id, toml, 30_000)
    }

    pub(super) fn detach_within(
        &self,
        test: &str,
        id: &str,
        toml: &str,
        milliseconds: u64,
    ) -> TestResult<String> {
        std::fs::write(self.root.join("detach.toml"), toml)?;
        let path = format!(
            "{}:{}",
            self.root.join("bin").display(),
            std::env::var("PATH")?
        );
        let output = Command::new(std::env::current_exe()?)
            .args(["--exact", test, "--nocapture"])
            .env(CHILD_MARKER, &self.root)
            .env("PATH", path)
            .env("VELNOR_FAKE_DIR", &self.root)
            .env(DETACH, id)
            .env(DETACH_MS, milliseconds.to_string())
            .output()?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        assert!(output.status.success(), "{stdout}");
        stdout
            .lines()
            .find_map(|line| line.strip_prefix("DETACH_RESULT "))
            .map(str::to_owned)
            .ok_or_else(|| format!("no detach result: {stdout}").into())
    }

    pub(super) fn held_job(&self, count: usize) -> u64 {
        self.wait_for_event("a held job", 60, |events| {
            events.iter().filter(|event| event.label == "jit").count() >= count
        });
        let job = super::checks::reservation(&self.gh("jit")[count - 1]);
        wait_for("its stand-in", 20, || self.pid_of(&jit_name(job)).is_some());
        job
    }

    pub(super) fn wait_census(&self, what: &str, config_text: &str, connections: &[&str]) {
        let hash = hex_sha256(config_text.as_bytes());
        let mut wanted: Vec<String> = connections.iter().map(|id| (*id).to_owned()).collect();
        wanted.sort();
        wait_for(what, 90, || {
            let Some(census) = self.census() else {
                return false;
            };
            let mut ids: Vec<String> = census["connections"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|tally| tally["id"].as_str().map(str::to_owned))
                .collect();
            ids.sort();
            census["config_sha256"] == hash.as_str() && ids == wanted
        });
    }

    pub(super) fn state_file(&self, name: &str) -> PathBuf {
        self.state.join(name)
    }

    pub(super) fn census(&self) -> Option<Value> {
        read_json(&self.state_file("rust-runner-census.json"))
    }

    pub(super) fn census_tally(&self, id: &str) -> Option<Value> {
        self.census()?["connections"]
            .as_array()?
            .iter()
            .find(|tally| tally["id"] == id)
            .cloned()
    }

    pub(super) fn age(&self, file: &str, milliseconds: i64) -> TestResult<()> {
        let path = self.state_file(file);
        let mut record = read_json(&path).ok_or("record")?;
        let stamp = record["updated_unix_ms"].as_u64().ok_or("stamp")?;
        record["updated_unix_ms"] = json!(stamp.saturating_add_signed(-milliseconds));
        std::fs::write(&path, record.to_string())?;
        Ok(())
    }

    pub(super) fn stamp_ahead(&self, file: &str, milliseconds: u64) -> TestResult<()> {
        let path = self.state_file(file);
        let mut record = read_json(&path).ok_or("record")?;
        record["updated_unix_ms"] = json!(now_unix_ms().saturating_add(milliseconds));
        std::fs::write(&path, record.to_string())?;
        Ok(())
    }

    pub(super) fn publish_empty_census(&self, config_text: &str) -> TestResult<()> {
        let census = json!({
            "config_sha256": hex_sha256(config_text.as_bytes()),
            "updated_unix_ms": now_unix_ms(),
            "drain": null,
            "reconciled": false,
            "connections": [],
        });
        self.write_marker(CENSUS, &census.to_string())
    }

    pub(super) fn hide_from_listing(&self) -> TestResult<()> {
        self.mutate_fake(|state| {
            state["hidden_from_listing"] = json!(true);
            Ok(())
        })
    }

    pub(super) fn write_marker(&self, relative: &str, token: &str) -> TestResult<()> {
        let path = self.state.join(relative);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let staging = path.with_extension("staging");
        std::fs::write(&staging, token)?;
        std::fs::rename(staging, path)?;
        Ok(())
    }
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}
