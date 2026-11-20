// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;

use crate::plan_diff::{aside, bind, data, plan, read, repository_with};

/// Check ids with a line break that would forge a plan output key or a workflow command.
const HOSTILE: [&str; 3] = [
    "x\nother_key=evil",
    "x\n::set-output name=other_key::evil",
    "x\n::add-mask::secret",
];

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
    for (index, id) in HOSTILE.iter().enumerate() {
        let block = format!(
            "\n[[checks]]\nid = {}\ntask = \"hostile\"\nselection = \"declared_inputs\"\ninputs = [\"hostile.txt\"]\n",
            toml::Value::String((*id).to_owned())
        );
        let name = format!("hostile-id-{index}");
        let (repo, base, head) = repository_with(&case, "pull_request", &name, &block);
        let request = bind(&read(&case.join("request.json")), &base, &head);
        let file = aside(&repo).join("plan-request.json");
        fs::write(&file, request).unwrap_or_else(|e| unreachable!("{e}"));
        let outputs = aside(&repo).join("outputs");
        let output = plan(&repo, &file, &outputs);
        let written = fs::read_to_string(&outputs).unwrap_or_default();
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(only_known_keys(&written), "{id:?}: {written}");
        assert!(!written.contains("evil"), "{id:?}: {written}");
        assert!(!written.contains("secret"), "{id:?}: {written}");
        assert!(!output.status.success(), "{id:?}: {written} {stderr}");
        assert!(stderr.contains("check_id:x%0A"), "{id:?}: {stderr}");
        fs::remove_dir_all(&repo).unwrap_or_else(|e| unreachable!("{e}"));
        fs::remove_dir_all(aside(&repo)).unwrap_or_else(|e| unreachable!("{e}"));
    }
}
