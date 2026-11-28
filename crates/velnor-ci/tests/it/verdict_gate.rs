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
    counted: Option<(&'static str, &'static str)>,
    raw: Option<Value>,
}

impl Gate {
    fn passing() -> Self {
        Self {
            event: "merge_group",
            reuse: "true",
            reused: " lint-alint rust.test rust.test-features-x ",
            alint: "skipped",
            counted: None,
            raw: None,
        }
    }

    fn run(&self) -> Output {
        let rust_outputs = self.raw.clone().unwrap_or_else(|| {
            self.counted.map_or_else(
                || json!({}),
                |(run, by_package)| {
                    json!({"tests_run": run, "tests_listed": "3", "tests_by_package": by_package})
                },
            )
        });
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
        Gate {
            event: "Merge_Group",
            ..Gate::passing()
        },
        Gate {
            reused: " lint-alint-x rust.test rust.test-features-x ",
            ..Gate::passing()
        },
        Gate {
            alint: "failure",
            ..Gate::passing()
        },
        Gate {
            alint: "cancelled",
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
fn reuse_counts_only_when_the_output_is_exactly_true() {
    for reuse in ["True", "true ", " true", "TRUE", "1", "yes", ""] {
        let gate = Gate {
            reuse,
            ..Gate::passing()
        };
        assert!(!gate.run().status.success(), "{reuse:?}");
    }
}

#[test]
fn counts_of_a_fully_reused_lane_are_judged_when_present() {
    let zero = "velnor-ci=0/3";
    for reuse in ["true", "false"] {
        let gate = Gate {
            alint: "success",
            counted: Some(("0", zero)),
            reuse,
            ..Gate::passing()
        };
        assert!(!gate.run().status.success(), "{reuse}");
    }
    let sound = Gate {
        counted: Some(("3", "velnor-ci=3/3")),
        ..Gate::passing()
    };
    assert!(sound.run().status.success());
}

#[test]
fn malformed_or_partial_counts_of_a_fully_reused_lane_refuse() {
    let cases = [
        json!({"tests_run": "0"}),
        json!({"tests_listed": "0"}),
        json!({"tests_by_package": "velnor-ci=3/3"}),
        json!({"tests_run": "3", "tests_listed": "3"}),
        json!({"tests_run": "3", "tests_listed": "3", "tests_by_package": ""}),
        json!({"tests_run": "0", "tests_listed": "3", "tests_by_package": "garbage"}),
        json!({"tests_run": "0", "tests_listed": "3", "tests_by_package": "velnor-ci=3/3"}),
        json!({"tests_ignored": "1"}),
        json!({"tests_run": "3", "tests_listed": "3", "tests_by_package": "velnor-ci=3/3", "tests_ignored": "x"}),
    ];
    for raw in cases {
        let gate = Gate {
            raw: Some(raw.clone()),
            ..Gate::passing()
        };
        assert!(!gate.run().status.success(), "{raw}");
    }
}

#[test]
fn counts_that_are_all_missing_or_all_empty_are_covered_by_the_reuse() {
    for raw in [
        json!({}),
        json!({"tests_run": "", "tests_listed": "", "tests_by_package": "", "tests_ignored": ""}),
    ] {
        let gate = Gate {
            raw: Some(raw.clone()),
            ..Gate::passing()
        };
        assert!(passes(&gate), "{raw}");
    }
}

#[test]
fn a_reused_job_that_ran_anyway_and_counted_tests_still_pass() {
    let gate = Gate {
        alint: "success",
        counted: Some(("3", "velnor-ci=3/3")),
        reuse: "false",
        reused: "",
        ..Gate::passing()
    };
    assert!(passes(&gate));
}
