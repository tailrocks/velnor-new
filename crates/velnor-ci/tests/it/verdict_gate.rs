// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::{Command, Output};

use serde_json::{Value, json};

const SPEC: &str = "--plan rust-plan --job rust-plan=always --job rust=always --job lint-alint=always --reuse lint-alint --reuse-tests rust=root:rust.test,rust.test-features-x --tests rust=root";
const RUN_URL: &str = "https://github.com/tailrocks/velnor-new/actions/runs/100";

struct Gate {
    event: &'static str,
    reuse: &'static str,
    reused: &'static str,
    alint: &'static str,
    counted: bool,
}

impl Gate {
    fn passing() -> Self {
        Self {
            event: "merge_group",
            reuse: "true",
            reused: " lint-alint rust.test rust.test-features-x ",
            alint: "skipped",
            counted: false,
        }
    }

    fn run(&self) -> Output {
        let rust_outputs = if self.counted {
            json!({"tests_run": "3", "tests_listed": "3", "tests_by_package": "velnor-ci=3/3"})
        } else {
            json!({})
        };
        let needs: Value = json!({
            "rust-plan": {"result": "success", "outputs": {
                "rust_root_tests": "run",
                "reuse": self.reuse,
                "reuse_run": RUN_URL,
                "reused": self.reused,
            }},
            "rust": {"result": "success", "outputs": rust_outputs},
            "lint-alint": {"result": self.alint},
        });
        Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["gate", "--spec-env", "GATE_SPEC"])
            .env("GATE_SPEC", SPEC)
            .env("NEEDS_JSON", needs.to_string())
            .env("GITHUB_EVENT_NAME", self.event)
            .output()
            .unwrap_or_else(|e| unreachable!("{e}"))
    }
}

fn passes(gate: &Gate) -> bool {
    let output = gate.run();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output.status.success() || {
        eprintln!("{text}");
        false
    }
}

#[test]
fn a_reused_job_and_reused_tests_pass_and_name_the_run_they_rest_on() {
    let output = Gate::passing().run();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    assert!(
        text.contains(&format!("reused: lint-alint passed in {RUN_URL}")),
        "{text}"
    );
    assert!(
        text.contains(&format!("reused: rust.test passed in {RUN_URL}")),
        "{text}"
    );
}

#[test]
fn a_skipped_job_or_absent_counts_fail_unless_the_verdict_covers_them() {
    let refused = [
        Gate {
            reuse: "false",
            ..Gate::passing()
        },
        Gate {
            reused: " rust.test rust.test-features-x ",
            ..Gate::passing()
        },
        Gate {
            reused: " lint-alint rust.test ",
            ..Gate::passing()
        },
        Gate {
            event: "pull_request",
            ..Gate::passing()
        },
        Gate {
            event: "push",
            ..Gate::passing()
        },
    ];
    for gate in &refused {
        assert!(
            !gate.run().status.success(),
            "{} {} {}",
            gate.event,
            gate.reuse,
            gate.reused
        );
    }
}

#[test]
fn a_reused_job_that_ran_anyway_and_counted_tests_still_pass() {
    let gate = Gate {
        alint: "success",
        counted: true,
        reuse: "false",
        reused: "",
        ..Gate::passing()
    };
    assert!(passes(&gate));
}
