// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![cfg(unix)]

use std::error::Error;
use std::fs;
use std::path::Path;
use std::process::Command;

use velnor_actions_contract::{StepKind, StepRole};
use velnor_actions_workflow_renderer::mbx_stats::mbx_stats_step;

const JOB: &str = "rust-demo";

const SESSION_ONE: &str = r#"{"type":"session_started","v":1,"session":"one"}
{"type":"wrapper_timing","v":1,"timing":{"adapter":"rustc"}}
{"type":"action","v":1,"outcome":{"kind":"bypass","reason":"compiler-query"},"duration_ns":0}
{"type":"action","v":1,"outcome":{"kind":"hit"},"crate_name":"serde","duration_ns":1}
{"type":"action","v":1,"outcome":{"kind":"hit"},"crate_name":"serde_derive","duration_ns":1}
{"type":"action","v":1,"outcome":{"kind":"miss"},"crate_name":"serde","duration_ns":1}
{"type":"action","v":1,"outcome":{"kind":"miss"},"crate_name":"blake3 build script","duration_ns":1}
{"type":"action","v":1,"outcome":{"kind":"unconsulted"},"crate_name":"syn","duration_ns":1}
{"type":"action","v":1,"outcome":{"kind":"bypass","reason":"unsupported-crate-type"},"crate_name":"cc","duration_ns":1}
{"type":"action","v":1,"outcome":{"kind":"bypass"},"duration_ns":1}
{"type":"action","v":1,"outcome":{"kind":"hit"},"crate_name":"velnor_a","duration_ns":1}
{"type":"action
"#;

const SESSION_TWO: &str = r#"{"type":"action","v":1,"outcome":{"kind":"miss"},"crate_name":"velnor_b","duration_ns":1}
{"type":"action","v":1,"outcome":{"kind":"miss"},"crate_name":"velnor_b","duration_ns":1}
{"type":"action","v":1,"outcome":{"kind":"bypass","reason":"standard-input"},"duration_ns":0}
{"type":"action","v":1,"outcome":{"kind":"bypass","reason":"compiler-query"},"duration_ns":0}
{"type":"session_finished","v":1,"stats":{"hits":3}}
"#;

fn run_step(
    cache_dir: &Path,
    summary: &Path,
    runner_temp: &Path,
) -> Result<String, Box<dyn Error>> {
    let step = mbx_stats_step(JOB, &["velnor_a".to_owned(), "velnor_b".to_owned()])?;
    let StepKind::Shell { run, env } = &step.kind else {
        return Err("the stats step is not a shell step".into());
    };
    let output = Command::new("sh")
        .arg("-c")
        .arg(run.get(2).ok_or("no script")?)
        .envs(env)
        .env("MBX_CACHE_DIR", cache_dir)
        .env("GITHUB_STEP_SUMMARY", summary)
        .env("RUNNER_TEMP", runner_temp)
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "the stats step failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

#[test]
fn stats_count_registry_and_workspace_outcomes_from_the_session_files() -> Result<(), Box<dyn Error>>
{
    let step = mbx_stats_step(JOB, &[])?;
    assert_eq!(step.role, Some(StepRole::MbxStats));
    assert_eq!(step.condition.as_deref(), Some("always()"));

    let dir = tempfile::tempdir()?;
    let cache = dir.path().join("mbx");
    let sessions = cache.join("actions/sessions/v1");
    fs::create_dir_all(&sessions)?;
    let summary = dir.path().join("summary");
    let temp = dir.path().join("temp");
    fs::create_dir_all(&temp)?;

    let none = run_step(&cache, &summary, &temp)?;
    assert_eq!(none, format!("mbx-stats job={JOB} sessions=0\n"));

    fs::write(sessions.join("1.jsonl"), SESSION_ONE)?;
    fs::write(sessions.join("2.jsonl"), SESSION_TWO)?;
    fs::write(sessions.join("2.lock"), "")?;
    let line = run_step(&cache, &summary, &temp)?;
    let expected = format!(
        "mbx-stats job={JOB} registry hit=2 miss=2 not_looked_up=1 bypassed=2 workspace hit=1 miss=2 not_looked_up=0 bypassed=0 bypassed_benign=3 nonhit=registry:bypass:cc,registry:bypass:unnamed,registry:miss:blake3_build_script,registry:miss:serde,registry:not_looked_up:syn,workspace:miss:velnor_b bypass=unrecorded:unnamed,unsupported-crate-type:cc\n"
    );
    assert_eq!(line, expected);
    assert_eq!(
        fs::read_to_string(&summary)?,
        format!("mbx-stats job={JOB} sessions=0\n{expected}")
    );
    Ok(())
}

#[test]
fn stats_bound_the_named_lists_to_forty_entries_on_one_line() -> Result<(), Box<dyn Error>> {
    let dir = tempfile::tempdir()?;
    let cache = dir.path().join("mbx");
    let sessions = cache.join("actions/sessions/v1");
    fs::create_dir_all(&sessions)?;
    let temp = dir.path().join("temp");
    fs::create_dir_all(&temp)?;
    let session = (1..=45)
        .map(|i| {
            format!(
                "{{\"type\":\"action\",\"v\":1,\"outcome\":{{\"kind\":\"bypass\",\"reason\":\"unsupported-crate-type\"}},\"crate_name\":\"b{i:02}\",\"duration_ns\":1}}\n"
            )
        })
        .collect::<Vec<_>>()
        .concat();
    fs::write(sessions.join("1.jsonl"), session)?;

    let line = run_step(&cache, &dir.path().join("summary"), &temp)?;

    let first_forty = |prefix: &str| {
        (1..=40)
            .map(|i| format!("{prefix}b{i:02}"))
            .collect::<Vec<_>>()
            .join(",")
    };
    let expected = format!(
        "mbx-stats job={JOB} registry hit=0 miss=0 not_looked_up=0 bypassed=45 workspace hit=0 miss=0 not_looked_up=0 bypassed=0 bypassed_benign=0 nonhit={},+5 bypass={},+5\n",
        first_forty("registry:bypass:"),
        first_forty("unsupported-crate-type:"),
    );
    assert_eq!(line, expected);
    assert_eq!(line.matches('\n').count(), 1);
    Ok(())
}
