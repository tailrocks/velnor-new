// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use velnor_local::conformance::{
    Values, differences, observations, report, run_values as run_values_of,
};
use velnor_local::io::run::log_values;

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

fn no_values(_: &str) -> Values {
    Values::new()
}

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
    let (lines, reproduced) = report(&logs, &stored, &no_values);
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
    let (lines, reproduced) = report(&logs, &stored, &no_values);
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
    assert!(report(&logs, &only, &no_values).1);
    let unknown = |_: &str| None;
    let (lines, reproduced) = report(&logs, &unknown, &no_values);
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
        differences(&stored, &observed, &Values::new()),
        [
            "claims.ac: GitHub only \"string\"",
            "claims.defaultRef: local only \"string\"",
        ]
    );
    let stored = |_: &str| Some(json!({"legs": {}}));
    let logs = [("1-push.log".to_owned(), "| no observation here".to_owned())];
    let (lines, reproduced) = report(&logs, &stored, &no_values);
    assert_eq!(lines, Vec::<String>::new());
    assert!(!reproduced);
}

fn run_values(git_ref: &str) -> Values {
    [
        ("repository", "tailrocks/velnor-lab"),
        ("ref", git_ref),
        ("defaultRef", "refs/heads/main"),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value.to_owned()))
    .collect()
}

#[test]
fn a_wrong_ref_fails_and_the_run_ref_passes() {
    let stored = json!({"claims": {"ref": "${ref}", "repository": "${repository}"}});
    let values = run_values("refs/pull/7/merge");
    let right =
        json!({"claims": {"ref": "refs/pull/7/merge", "repository": "tailrocks/velnor-lab"}});
    let wrong = json!({"claims": {"ref": "refs/heads/main", "repository": "tailrocks/velnor-lab"}});
    assert_eq!(differences(&stored, &right, &values), Vec::<String>::new());
    assert_eq!(
        differences(&stored, &wrong, &values),
        ["claims.ref: GitHub \"refs/pull/7/merge\", local \"refs/heads/main\""]
    );
}

#[test]
fn a_wrong_repository_or_default_ref_fails_and_a_scope_list_is_compared_by_value() {
    let stored = json!({
        "cache_scopes": [{"permission": 3, "scope": "${ref}"}, {"permission": 1, "scope": "${defaultRef}"}],
        "claims": {"defaultRef": "${defaultRef}", "repository": "${repository}"}
    });
    let values = run_values("refs/heads/feature");
    let observed = json!({
        "cache_scopes": [{"permission": 3, "scope": "refs/heads/feature"}, {"permission": 1, "scope": "refs/heads/develop"}],
        "claims": {"defaultRef": "refs/heads/develop", "repository": "someone/else"}
    });
    assert_eq!(
        differences(&stored, &observed, &values),
        [
            "cache_scopes: GitHub [{\"permission\":3,\"scope\":\"refs/heads/feature\"},{\"permission\":1,\"scope\":\"refs/heads/main\"}], local [{\"permission\":3,\"scope\":\"refs/heads/feature\"},{\"permission\":1,\"scope\":\"refs/heads/develop\"}]",
            "claims.defaultRef: GitHub \"refs/heads/main\", local \"refs/heads/develop\"",
            "claims.repository: GitHub \"tailrocks/velnor-lab\", local \"someone/else\"",
        ]
    );
}

#[test]
fn a_volatile_field_is_compared_by_type_so_a_different_timestamp_passes() {
    let stored = json!({"claims": {"exp": "number", "iss": "string", "job_id": "string"}});
    let observed = json!({"claims": {"exp": 1_700_000_000, "iss": "https://token.example", "job_id": "string"}});
    assert_eq!(
        differences(&stored, &observed, &Values::new()),
        Vec::<String>::new()
    );
    let wrong_type = json!({"claims": {"exp": "string", "iss": "string", "job_id": "string"}});
    assert_eq!(
        differences(&stored, &wrong_type, &Values::new()),
        ["claims.exp: GitHub \"number\", local \"string\""]
    );
}

#[test]
fn the_run_values_come_from_the_payload_the_driver_wrote() {
    let repository = json!({"full_name": "tailrocks/velnor-lab", "default_branch": "main"});
    let push = json!({"repository": repository, "ref": "refs/heads/probe-1-events"});
    assert_eq!(
        run_values_of(&push),
        run_values("refs/heads/probe-1-events")
    );
    let open = json!({"repository": repository, "number": 7,
        "pull_request": {"number": 7, "merged": false, "base": {"ref": "main"}}});
    assert_eq!(run_values_of(&open), run_values("refs/pull/7/merge"));
    let merged = json!({"repository": repository, "number": 7,
        "pull_request": {"number": 7, "merged": true, "base": {"ref": "main"}}});
    assert_eq!(run_values_of(&merged), run_values("refs/heads/main"));
}

const PLAIN: &str = r#"[probe / observe] Running: Run ./.github/actions/probe
{
  "legs": {
    "default-branch": {
      "hit": "true",
      "matched": "lab-probe-<tag>-a"
    }
  },
  "probe": "cache-keys"
}
[probe / other] Running: Run sleep 1
"#;

#[test]
fn an_unprefixed_probe_observation_in_a_job_log_is_read() {
    assert_eq!(
        observations(PLAIN),
        [
            json!({"legs": {"default-branch": {"hit": "true", "matched": "lab-probe-<tag>-a"}}, "probe": "cache-keys"})
        ]
    );
    let single = r#"{"legs": {"default-branch": {"hit": "true"}}, "probe": "cache-keys"}"#;
    assert_eq!(observations(single).len(), 1);
}

#[test]
fn ordinary_json_without_the_probe_key_is_not_an_observation() {
    let log = "{\n  \"name\": \"velnor\",\n  \"legs\": {}\n}\n{\"items\": [1, 2]}\n";
    assert_eq!(observations(log), Vec::<serde_json::Value>::new());
}

#[test]
fn a_log_under_logs_jobs_takes_the_values_of_its_run() {
    let dir = crate::support::temp_dir("conformance-job-values");
    std::fs::create_dir_all(dir.join("runs")).unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::create_dir_all(dir.join("events")).unwrap_or_else(|e| unreachable!("{e}"));
    let saved = serde_json::to_string(&run_values("refs/pull/7/merge")).unwrap_or_default();
    std::fs::write(dir.join("runs/5.json"), saved).unwrap_or_else(|e| unreachable!("{e}"));
    let push = json!({"repository": {"full_name": "tailrocks/velnor-lab", "default_branch": "main"}, "ref": "refs/heads/probe-1-events"});
    std::fs::write(dir.join("events/1-push.json"), push.to_string())
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(
        log_values(&dir, "jobs/5-7.log"),
        run_values("refs/pull/7/merge")
    );
    assert_eq!(
        log_values(&dir, "1-push.log"),
        run_values("refs/heads/probe-1-events")
    );
    assert_eq!(log_values(&dir, "jobs/9-7.log"), Values::new());
}

fn observation_log(hit: &str) -> String {
    format!(
        "{{\"legs\": {{\"default-branch\": {{\"hit\": \"{hit}\"}}}}, \"probe\": \"cache-keys\"}}"
    )
}

#[test]
fn a_wrong_observation_of_a_leg_is_not_hidden_by_a_later_log() {
    let mut logs = vec![
        ("10-push.log".to_owned(), observation_log("false")),
        ("9-push.log".to_owned(), observation_log("true")),
    ];
    velnor_local::io::run::order_logs(&mut logs);
    let stored = |_: &str| Some(json!({"legs": {"default-branch": {"hit": "true"}}}));
    let (lines, reproduced) = report(&logs, &stored, &no_values);
    assert!(!reproduced, "{lines:?}");
    assert_eq!(
        lines,
        ["cache-keys / default-branch (9-push.log, 10-push.log): conflicting observations"]
    );
}

#[test]
fn two_logs_that_agree_on_a_leg_reproduce_it() {
    let logs = [
        ("9-push.log".to_owned(), observation_log("true")),
        ("10-push.log".to_owned(), observation_log("true")),
    ];
    let stored = |_: &str| Some(json!({"legs": {"default-branch": {"hit": "true"}}}));
    let (lines, reproduced) = report(&logs, &stored, &no_values);
    assert_eq!(
        lines,
        ["cache-keys / default-branch (9-push.log, 10-push.log): reproduced"]
    );
    assert!(reproduced);
}

#[test]
fn logs_are_ordered_by_run_number_then_name() {
    let mut logs: Vec<(String, String)> =
        ["10-push.log", "jobs/10-7.log", "9-push.log", "jobs/9-7.log"]
            .into_iter()
            .map(|name| (name.to_owned(), String::new()))
            .collect();
    velnor_local::io::run::order_logs(&mut logs);
    let names: Vec<&str> = logs.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        ["9-push.log", "jobs/9-7.log", "10-push.log", "jobs/10-7.log"]
    );
}
