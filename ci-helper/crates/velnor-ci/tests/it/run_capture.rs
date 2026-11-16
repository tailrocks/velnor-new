// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[test]
fn completed_observations_survive_a_descendant_holding_the_stream_open() {
    for (name, active, held) in [
        ("held", true, true),
        ("empty", false, true),
        ("eof", true, false),
    ] {
        let temp =
            std::env::temp_dir().join(format!("velnor-capture-{}-{name}", std::process::id()));
        let program = fixture(&temp, active, held).expect("fixture");
        let started = Instant::now();
        let mut child = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args([
                "run",
                "--",
                program.to_str().expect("path"),
                "nextest",
                "run",
            ])
            .current_dir(&temp)
            .env("RUNNER_TEMP", &temp)
            .env("GITHUB_OUTPUT", temp.join("outputs"))
            .env("VELNOR_CI_RUN_PUMP_GRACE_MS", "20")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("run helper");
        let status = loop {
            if let Some(status) = child.try_wait().expect("wait") {
                break status;
            }
            assert!(
                started.elapsed() < Duration::from_secs(2),
                "wrapper exceeded bounded wait"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(status.success());
        let text = fs::read_to_string(temp.join("velnor-invocations.jsonl")).expect("sidecar");
        let invocation: serde_json::Value = serde_json::from_str(text.trim()).expect("record");
        assert_eq!(
            invocation["progress"].as_array().expect("progress").len(),
            usize::from(active),
            "{name}: completed progress lost"
        );
        assert_eq!(
            invocation["artifacts"].as_array().expect("artifacts").len(),
            usize::from(active)
        );
        assert_eq!(
            invocation["tests"].as_array().expect("tests").len(),
            usize::from(active)
        );
        assert_eq!(invocation["capture_complete"], !held);
        assert_eq!(
            fs::read_to_string(temp.join("outputs")).unwrap_or_default(),
            if active { "tests_run=17\n" } else { "" }
        );
        let decision = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
            .args(["cache", "decide", "--target-dir", "target"])
            .current_dir(&temp)
            .env("RUNNER_TEMP", &temp)
            .env("GITHUB_OUTPUT", temp.join("cache-outputs"))
            .output()
            .expect("cache decision");
        if active {
            assert!(decision.status.success(), "{decision:?}");
            assert_eq!(
                fs::read_to_string(temp.join("cache-outputs")).expect("cache output"),
                "save=true\n"
            );
        } else {
            assert!(
                !decision.status.success(),
                "incomplete capture cannot establish zero activity"
            );
            assert!(String::from_utf8_lossy(&decision.stderr).contains("incomplete"));
        }
        fs::remove_dir_all(temp).expect("cleanup");
    }
}

fn fixture(
    temp: &std::path::Path,
    active: bool,
    held: bool,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    if temp.exists() {
        fs::remove_dir_all(temp)?;
    }
    fs::create_dir_all(temp)?;
    let artifact = serde_json::json!({
        "reason": "compiler-artifact", "package_id": "demo 0.1.0",
        "manifest_path": "/demo/Cargo.toml", "target": {
            "name": "demo", "kind": ["lib"], "crate_types": ["rlib"], "src_path": "/demo/src/lib.rs"
        }, "profile": {"opt_level": "0", "debuginfo": 0, "debug_assertions": true,
            "overflow_checks": true, "test": false}, "features": [], "filenames": [], "fresh": false
    });
    let records = format!(
        "   Compiling demo v0.1.0\n{artifact}\nPASS [0.125s] demo test\n     Summary [0.2s] 17 tests run: 17 passed, 0 skipped\n"
    );
    fs::write(
        temp.join("events"),
        if active { &records } else { "ready\n" },
    )?;
    let program = temp.join("cargo-nextest");
    fs::write(
        &program,
        format!(
            "#!/bin/sh\ncat events >&2\ni=0\nuntil grep -q '{}' velnor-run.log; do i=$((i+1)); [ $i -lt 500 ] || exit 90; sleep 0.01; done\n{}\n",
            if active { "17 tests run" } else { "ready" },
            if held { "sleep 3 &" } else { ":" }
        ),
    )?;
    fs::set_permissions(&program, fs::Permissions::from_mode(0o755))?;
    Ok(program)
}
