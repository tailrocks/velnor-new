// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::process::Command;

use velnor_ci::preview::sealed::{ALLOWED_ENV, plan};

use crate::preview_world::sha;
use crate::support::fresh_dir;

fn vars(temp: &str, expected: &str) -> BTreeMap<String, String> {
    [
        ("RUNNER_TEMP", temp),
        ("VERIFIED_LINUX_SHA256", expected),
        ("PRODUCER_RUN_ID", "11"),
        ("PRODUCER_RUN_ATTEMPT", "2"),
        ("PRODUCER_ARTIFACT_ID", "33"),
        ("PUBLISHER_ARTIFACT_ID", "44"),
        ("GH_TOKEN", "token"),
        ("HOME", "/home/runner"),
        ("PATH", "/usr/bin:/bin"),
        ("VELNOR_Q_GH", "/opt/gh"),
        ("LD_PRELOAD", "/tmp/evil.so"),
        ("PYTHONPATH", "/tmp/evil"),
        ("VERIFIED_LINUX_SHA256_EXTRA", "x"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_owned(), v.to_owned()))
    .collect()
}

#[test]
fn the_plan_is_the_legacy_command_line() {
    let planned = plan(&vars("/runner/temp", &sha(b"sdk"))).unwrap_or_else(|e| unreachable!("{e}"));
    let args: Vec<OsString> = [
        "image-maintenance",
        "verify-mac-attestation",
        "--producer-run-id",
        "11",
        "--producer-run-attempt",
        "2",
        "--producer-artifact-id",
        "33",
        "--publisher-artifact-id",
        "44",
        "--assets-dir",
        "/runner/temp/runner-family-release/assets",
        "--runner-temp",
        "/runner/temp",
    ]
    .iter()
    .map(OsString::from)
    .collect();
    assert_eq!(planned.args, args);
    assert_eq!(
        planned.source,
        Path::new("/runner/temp/source-preview-runtime/linux")
    );
    assert_eq!(planned.expected_sha256, sha(b"sdk"));
}

#[test]
fn only_the_allowed_environment_crosses_the_exec() {
    let planned = plan(&vars("/runner/temp", &sha(b"sdk"))).unwrap_or_else(|e| unreachable!("{e}"));
    let keys: Vec<&str> = planned.env.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        ["GH_TOKEN", "HOME", "PATH", "RUNNER_TEMP", "VELNOR_Q_GH"]
    );
    assert!(keys.iter().all(|k| ALLOWED_ENV.contains(k)));
}

#[test]
fn the_plan_is_refused_without_an_absolute_temp_a_full_digest_or_every_input() {
    let mut cases: Vec<BTreeMap<String, String>> = Vec::new();
    cases.push(vars("relative/temp", &sha(b"sdk")));
    cases.push(vars("/runner/temp", &sha(b"sdk").to_uppercase()));
    cases.push(vars("/runner/temp", &sha(b"sdk")[..63]));
    for key in [
        "RUNNER_TEMP",
        "VERIFIED_LINUX_SHA256",
        "PRODUCER_RUN_ID",
        "PRODUCER_RUN_ATTEMPT",
        "PRODUCER_ARTIFACT_ID",
        "PUBLISHER_ARTIFACT_ID",
    ] {
        let mut missing = vars("/runner/temp", &sha(b"sdk"));
        missing.remove(key);
        cases.push(missing);
    }
    for case in cases {
        assert!(plan(&case).is_err());
    }
}

fn run(env: &[(&str, &str)]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_velnor-ci"))
        .arg("verify-mac-proof")
        .env_clear()
        .envs(env.iter().copied())
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"))
}

#[test]
fn the_command_exits_1_without_its_environment() {
    let out = run(&[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("source preview executable rejected"));
}

#[test]
fn a_missing_sdk_is_refused() {
    let root = fs::canonicalize(fresh_dir("velnor-ci-sealed", "missing"))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let temp = root.display().to_string();
    let out = run(&[
        ("RUNNER_TEMP", &temp),
        ("VERIFIED_LINUX_SHA256", &sha(b"sdk")),
        ("PRODUCER_RUN_ID", "1"),
        ("PRODUCER_RUN_ATTEMPT", "1"),
        ("PRODUCER_ARTIFACT_ID", "1"),
        ("PUBLISHER_ARTIFACT_ID", "1"),
    ]);
    assert_eq!(out.status.code(), Some(1));
}

#[cfg(target_os = "linux")]
mod linux {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::*;

    fn sdk(name: &str, script: &str) -> (String, std::path::PathBuf) {
        let root = fs::canonicalize(fresh_dir("velnor-ci-sealed", name))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let dir = root.join("source-preview-runtime");
        fs::create_dir(&dir).unwrap_or_else(|e| unreachable!("{e}"));
        fs::write(dir.join("linux"), script).unwrap_or_else(|e| unreachable!("{e}"));
        fs::set_permissions(dir.join("linux"), fs::Permissions::from_mode(0o700))
            .unwrap_or_else(|e| unreachable!("{e}"));
        (root.display().to_string(), root)
    }

    fn env<'a>(temp: &'a str, digest: &'a str) -> Vec<(&'a str, &'a str)> {
        vec![
            ("RUNNER_TEMP", temp),
            ("VERIFIED_LINUX_SHA256", digest),
            ("PRODUCER_RUN_ID", "11"),
            ("PRODUCER_RUN_ATTEMPT", "2"),
            ("PRODUCER_ARTIFACT_ID", "33"),
            ("PUBLISHER_ARTIFACT_ID", "44"),
            ("GH_TOKEN", "token"),
            ("LD_PRELOAD", "/tmp/evil.so"),
        ]
    }

    #[test]
    fn the_verified_sdk_runs_from_a_sealed_copy_with_the_legacy_arguments_and_only_allowed_environment()
     {
        let script = "#!/bin/sh\nprintf '%s\\n' \"$0\" \"$@\" \"token=$GH_TOKEN\" \"preload=${LD_PRELOAD-unset}\" > \"$RUNNER_TEMP/seen\"\n";
        let (temp, root) = sdk("ok", script);
        let out = run(&env(&temp, &sha(script.as_bytes())));
        assert_eq!(
            out.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let seen = fs::read_to_string(root.join("seen")).unwrap_or_default();
        let lines: Vec<&str> = seen.lines().collect();
        assert!(lines[0].starts_with("/proc/self/fd/"));
        assert_eq!(lines[1..3], ["image-maintenance", "verify-mac-attestation"]);
        assert_eq!(lines[lines.len() - 2..], ["token=token", "preload=unset"]);
    }

    #[test]
    fn an_sdk_that_does_not_match_the_verified_digest_is_not_run() {
        let (temp, root) = sdk("tampered", "#!/bin/sh\ntouch \"$RUNNER_TEMP/ran\"\n");
        let out = run(&env(&temp, &sha(b"another sdk")));
        assert_eq!(out.status.code(), Some(1));
        assert!(!root.join("ran").exists());
    }

    #[test]
    fn a_symbolic_link_in_place_of_the_sdk_is_not_followed() {
        let (temp, root) = sdk("link", "#!/bin/sh\ntouch \"$RUNNER_TEMP/ran\"\n");
        fs::rename(root.join("source-preview-runtime/linux"), root.join("real"))
            .unwrap_or_else(|e| unreachable!("{e}"));
        symlink(root.join("real"), root.join("source-preview-runtime/linux"))
            .unwrap_or_else(|e| unreachable!("{e}"));
        let out = run(&env(
            &temp,
            &sha(b"#!/bin/sh\ntouch \"$RUNNER_TEMP/ran\"\n"),
        ));
        assert_eq!(out.status.code(), Some(1));
        assert!(!root.join("ran").exists());
    }
}
