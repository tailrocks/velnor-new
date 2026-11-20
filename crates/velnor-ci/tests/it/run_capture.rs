// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const TEST_OUTPUT: &str = "tests_run=17\ntests_listed=17\n";

#[test]
fn completed_observations_survive_a_descendant_holding_the_stream_open() {
    for (name, active, held) in [
        ("held", true, true),
        ("empty", false, true),
        ("eof", true, false),
    ] {
        let temp = crate::scratch_path("velnor-capture", name);
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
            .env("CARGO_HOME", temp.join("cargo-home"))
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
            crate::run_nextest::without_built(
                &fs::read_to_string(temp.join("outputs")).unwrap_or_default()
            ),
            if active && !held { TEST_OUTPUT } else { "" }
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
                "save=true\nsources=true\n"
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

pub(crate) fn fixture(
    temp: &std::path::Path,
    active: bool,
    held: bool,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    if temp.exists() {
        fs::remove_dir_all(temp)?;
    }
    fs::create_dir_all(temp.join("target/debug/deps"))?;
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(temp)
        .output()?;
    if !git.status.success() {
        return Err(format!("cannot initialize capture fixture: {git:?}").into());
    }
    fs::write(temp.join("Cargo.lock"), "version = 4\n")?;
    let artifact = serde_json::json!({
        "reason": "compiler-artifact", "package_id": "demo 0.1.0",
        "manifest_path": "/demo/Cargo.toml", "target": {
            "name": "demo", "kind": ["lib"], "crate_types": ["rlib"], "src_path": "/demo/src/lib.rs"
        }, "profile": {"opt_level": "0", "debuginfo": 0, "debug_assertions": true,
            "overflow_checks": true, "test": false}, "features": [], "filenames": ["target/debug/deps/libdemo-abc.rmeta"], "fresh": false
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

#[test]
fn recorded_target_directory_uses_only_explicit_cargo_settings() {
    for (name, options, environment, expected) in [
        (
            "separate",
            vec!["--target-dir", "explicit"],
            Some("other"),
            Some("explicit"),
        ),
        (
            "inline",
            vec!["--target-dir=explicit"],
            Some("other"),
            Some("explicit"),
        ),
        (
            "environment",
            vec![],
            Some("environment"),
            Some("environment"),
        ),
        ("default", vec![], None, None),
        (
            "manifest",
            vec!["--manifest-path", "member/Cargo.toml"],
            None,
            None,
        ),
        (
            "config",
            vec!["--config", "build.target-dir='configured'"],
            None,
            None,
        ),
    ] {
        let temp = crate::scratch_path("velnor-directory", name);
        let program = fixture(&temp, true, false).expect("fixture");
        let temp = temp.canonicalize().expect("canonical root");
        fs::write(&program, "#!/bin/sh\ncat events\n").expect("program");
        fs::create_dir(temp.join(".cargo")).expect("config directory");
        fs::write(
            temp.join(".cargo/config.toml"),
            "[build]\ntarget-dir = 'configured'\n",
        )
        .expect("config");
        let mut argv = vec![
            program.to_str().expect("path"),
            "+stable",
            "test",
            "--config=build.jobs=1",
        ];
        argv.extend(options);
        argv.extend([
            "--",
            "--target-dir",
            "trailing",
            "--config=build.jobs=2",
            "+nightly",
        ]);
        let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
        command
            .args(["run", "--"])
            .args(&argv)
            .current_dir(&temp)
            .env("RUNNER_TEMP", &temp)
            .env_remove("CARGO_TARGET_DIR");
        if let Some(directory) = environment {
            command.env("CARGO_TARGET_DIR", directory);
        }
        let output = command.output().expect("run helper");
        assert!(output.status.success(), "{name}: {output:?}");
        let text = fs::read_to_string(temp.join("velnor-invocations.jsonl")).expect("sidecar");
        let invocation: serde_json::Value = serde_json::from_str(text.trim()).expect("record");
        assert_recorded_settings(&invocation, &argv, &temp, expected, name);
        fs::remove_dir_all(temp).expect("cleanup");
    }
}

fn assert_recorded_settings(
    invocation: &serde_json::Value,
    argv: &[&str],
    temp: &std::path::Path,
    expected: Option<&str>,
    name: &str,
) {
    assert_eq!(invocation["argv"], serde_json::json!(argv), "{name}");
    assert_eq!(
        invocation["target_directory"],
        serde_json::json!(expected.map(|directory| temp.join(directory))),
        "{name}"
    );
    assert_eq!(
        invocation["execution"]["cargo-config-arguments"],
        if name == "config" {
            "[\"build.jobs=1\", \"build.target-dir='configured'\"]"
        } else {
            "[\"build.jobs=1\"]"
        },
        "{name}"
    );
    assert_eq!(
        invocation["execution"]["toolchain-argument"], "+stable",
        "{name}"
    );
    assert_eq!(
        invocation["artifacts"][0]["filenames"],
        serde_json::json!(["target/debug/deps/libdemo-abc.rmeta"]),
        "{name}"
    );
}

#[cfg(unix)]
#[test]
fn unrelated_non_unicode_environment_preserves_the_child_result_and_record() {
    use std::os::unix::ffi::OsStringExt;

    let temp = std::env::temp_dir().join(format!("velnor-non-unicode-{}", std::process::id()));
    let program = fixture(&temp, true, false).expect("fixture");
    fs::write(
        &program,
        "#!/bin/sh\ntest \"$UNRELATED\" = \"$(printf '\\377')\" || exit 91\ncat events\n",
    )
    .expect("program");
    let argv = [program.to_str().expect("path"), "test"];
    let output = Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .args(["run", "--"])
        .args(argv)
        .current_dir(&temp)
        .env("RUNNER_TEMP", &temp)
        .env("UNRELATED", std::ffi::OsString::from_vec(vec![255]))
        .env(std::ffi::OsString::from_vec(vec![b'U', 255]), "unrelated")
        .output()
        .expect("run helper");
    assert!(output.status.success(), "{output:?}");
    let text = fs::read_to_string(temp.join("velnor-invocations.jsonl")).expect("sidecar");
    let invocation: serde_json::Value = serde_json::from_str(text.trim()).expect("record");
    assert_eq!(invocation["argv"], serde_json::json!(argv));
    assert_eq!(invocation["capture_complete"], true);
    assert_eq!(
        invocation["artifacts"].as_array().expect("artifacts").len(),
        1
    );
    fs::remove_dir_all(temp).expect("cleanup");
}
