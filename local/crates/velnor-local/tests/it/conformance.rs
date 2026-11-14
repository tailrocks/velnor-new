// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_local::conformance::{differences, observations, report};

const LOG: &str = r#"[probe / observe] Running: Run ./.github/actions/probe
| {
|   "legs": {
|     "default-branch": {
[probe / other] Running: Run sleep 1
|       "hit": "true",
|       "matched": "lab-probe-<tag>-a"
|     }
|   },
|   "probe": "cache-keys"
| }
| {
|   "not": "an observation"
| }
"#;

#[test]
fn an_observation_is_read_from_the_job_log_between_the_lines_of_other_jobs() {
    let found = observations(LOG);
    assert_eq!(
        found,
        [
            json!({"legs": {"default-branch": {"hit": "true", "matched": "lab-probe-<tag>-a"}}, "probe": "cache-keys"})
        ]
    );
}

#[test]
fn each_stored_leg_is_reported_as_reproduced_different_or_not_observed() {
    let stored = |probe: &str| -> Option<Value> {
        (probe == "cache-keys").then(|| {
            json!({"legs": {
                "default-branch": {"hit": "true", "matched": "lab-probe-<tag>-a"},
                "pull_request": {"hit": ""}
            }})
        })
    };
    let logs = [("1-workflow_dispatch.log".to_owned(), LOG.to_owned())];
    let (lines, reproduced) = report(&logs, &stored);
    assert_eq!(
        lines,
        [
            "cache-keys / default-branch (1-workflow_dispatch.log): reproduced",
            "cache-keys / pull_request: not observed",
        ]
    );
    assert!(!reproduced);
    let changed = LOG.replace(r#""hit": "true""#, r#""hit": "false""#);
    let logs = [("2-push.log".to_owned(), changed)];
    let (lines, reproduced) = report(&logs, &stored);
    assert_eq!(
        lines[0],
        "cache-keys / default-branch (2-push.log): 1 differences\n    hit: GitHub \"true\", local \"false\""
    );
    assert!(!reproduced);
    let only = |probe: &str| {
        stored(probe).map(|mut file| {
            file["legs"]
                .as_object_mut()
                .map(|legs| legs.remove("pull_request"));
            file
        })
    };
    let logs = [("1-workflow_dispatch.log".to_owned(), LOG.to_owned())];
    assert!(report(&logs, &only).1);
    let unknown = |_: &str| None;
    let (lines, reproduced) = report(&logs, &unknown);
    assert_eq!(
        lines,
        ["cache-keys / default-branch: not stored in conformance/cache-keys.json"]
    );
    assert!(!reproduced);
}

#[test]
fn a_key_on_one_side_only_is_a_difference_and_a_run_with_no_observation_reproduces_nothing() {
    let stored = json!({"claims": {"ac": "string", "sub": "string"}});
    let observed = json!({"claims": {"sub": "string", "defaultRef": "string"}});
    assert_eq!(
        differences(&stored, &observed),
        [
            "claims.ac: GitHub only \"string\"",
            "claims.defaultRef: local only \"string\"",
        ]
    );
    let stored = |_: &str| Some(json!({"legs": {}}));
    let logs = [("1-push.log".to_owned(), "| no observation here".to_owned())];
    let (lines, reproduced) = report(&logs, &stored);
    assert_eq!(lines, Vec::<String>::new());
    assert!(!reproduced);
}
