// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::plan_diff::{aside, bind, data, plan, read, repository_with};

const HOSTILE: [&str; 3] = [
    "x\nother_key=evil",
    "x\n::set-output name=other_key::evil",
    "x\n::add-mask::secret",
];

fn append_checks(repo: &Path) {
    let path = repo.join(".velnor/config.toml");
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap_or_else(|e| unreachable!("{e}"));
    for id in HOSTILE {
        let block = format!(
            "\n[[checks]]\nid = {}\ntask = \"hostile\"\nselection = \"declared_inputs\"\ninputs = [\"hostile.txt\"]\n",
            toml::Value::String(id.to_owned())
        );
        file.write_all(block.as_bytes())
            .unwrap_or_else(|e| unreachable!("{e}"));
    }
}

fn only_known_keys(written: &str) -> bool {
    written.lines().all(|line| {
        ["plan_id", "run_key", "not_affected_jobs", "qualification_"]
            .iter()
            .any(|key| line.starts_with(key))
    })
}

#[test]
fn a_check_id_with_a_line_break_cannot_add_a_plan_output() {
    let case = data().join("pr-one-crate");
    let (repo, base, head) = repository_with(&case, "pull_request", "hostile-id", append_checks);
    let request = bind(&read(&case.join("request.json")), &base, &head);
    let file = aside(&repo).join("plan-request.json");
    fs::write(&file, request).unwrap_or_else(|e| unreachable!("{e}"));
    let outputs = aside(&repo).join("outputs");
    let output = plan(&repo, &file, &outputs);
    let written = fs::read_to_string(&outputs).unwrap_or_default();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(only_known_keys(&written), "{written}");
    assert!(!written.contains("evil"), "{written}");
    assert!(!output.status.success(), "{written} {stderr}");
    assert!(stderr.contains("check_id:x%0A"), "{stderr}");
    fs::remove_dir_all(&repo).unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(aside(&repo)).unwrap_or_else(|e| unreachable!("{e}"));
}

#[test]
fn a_lane_job_id_with_a_line_break_cannot_add_a_plan_output() {
    let case = data().join("pr-one-crate");
    let (repo, base, head) = repository_with(&case, "pull_request", "hostile-job", |_| ());
    let mut value: serde_json::Value =
        serde_json::from_str(&bind(&read(&case.join("request.json")), &base, &head))
            .unwrap_or_else(|e| unreachable!("{e}"));
    let routes = value["lane_inventory"]["routes"]
        .as_array_mut()
        .unwrap_or_else(|| unreachable!("routes"));
    let victim = routes
        .iter_mut()
        .find(|route| route["logical_job_id"] == "rust-velnor-gen")
        .unwrap_or_else(|| unreachable!("route"));
    victim["job_id"] = "rust-velnor-gen\nother_key=evil".into();
    let file = aside(&repo).join("plan-request.json");
    fs::write(&file, value.to_string()).unwrap_or_else(|e| unreachable!("{e}"));
    let outputs = aside(&repo).join("outputs");
    let output = plan(&repo, &file, &outputs);
    let written = fs::read_to_string(&outputs).unwrap_or_default();
    assert!(only_known_keys(&written), "{written}");
    assert!(!output.status.success(), "{written}");
    fs::remove_dir_all(&repo).unwrap_or_else(|e| unreachable!("{e}"));
    fs::remove_dir_all(aside(&repo)).unwrap_or_else(|e| unreachable!("{e}"));
}
