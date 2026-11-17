// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Value, json};
use std::fs;
use std::process::{Command, ExitStatus};

#[test]
fn archive_witness_records_only_created_or_changed_archives_in_the_resolved_cargo_home() {
    for (name, home, initial, action, expected) in [
        ("unchanged", "explicit", "archive", ":", json!([])),
        (
            "created",
            "explicit",
            "missing",
            "printf new > \"$ARCHIVE\"",
            json!(["registry/demo-0.1.0.crate"]),
        ),
        (
            "changed",
            "relative",
            "archive",
            "printf longer-content > \"$ARCHIVE\"",
            json!(["registry/demo-0.1.0.crate"]),
        ),
        (
            "default",
            "default",
            "missing",
            "printf new > \"$ARCHIVE\"",
            json!(["registry/demo-0.1.0.crate"]),
        ),
        (
            "two-registries",
            "explicit",
            "missing",
            "mkdir -p \"$CACHE/other\"; printf new > \"$ARCHIVE\"; printf new > \"$CACHE/other/demo-0.1.0.crate\"",
            json!(["other/demo-0.1.0.crate", "registry/demo-0.1.0.crate"]),
        ),
        (
            "unrelated",
            "explicit",
            "missing",
            "printf ignored > \"$ARCHIVE.txt\"",
            json!([]),
        ),
    ] {
        let (status, invocation) =
            recording(name, home, initial, action, 0, &[]).expect("recording");
        assert!(status.success(), "{name}: {status:?}");
        assert_eq!(invocation["verified_download_archives"], expected, "{name}");
        assert_eq!(invocation["capture_complete"], true, "{name}");
    }
}

#[test]
fn an_unreadable_archive_cache_records_an_unavailable_witness_and_keeps_the_child_exit_code() {
    for (name, initial, action) in [
        ("unavailable-before", "file", ":"),
        (
            "unavailable-after",
            "after-file",
            "mkdir -p \"$(dirname \"$CACHE\")\"; printf blocked > \"$CACHE\"",
        ),
    ] {
        let (status, invocation) =
            recording(name, "explicit", initial, action, 7, &[]).expect("recording");
        assert_eq!(status.code(), Some(7));
        assert_eq!(invocation["verified_download_archives"], Value::Null);
        assert_eq!(invocation["capture_complete"], true);
    }
}

#[test]
fn known_cargo_commands_without_downloads_do_not_read_the_archive_cache() {
    for (name, args, available) in [
        ("fmt", vec!["fmt", "--check"], true),
        ("offline", vec!["fetch", "--offline"], true),
        ("frozen", vec!["+nightly", "metadata", "--frozen"], true),
        ("offline-build", vec!["build", "--offline"], false),
        ("offline-test", vec!["test", "--offline"], false),
        ("nextest", vec!["nextest", "run", "--offline"], false),
        ("online", vec!["build"], false),
        ("value", vec!["build", "--target-dir", "--offline"], false),
        ("config-value", vec!["--config", "fmt", "build"], false),
        ("separator", vec!["build", "--", "--offline"], false),
        ("unknown", vec!["custom", "--offline"], false),
    ] {
        let (status, invocation) =
            recording(name, "explicit", "file", ":", 0, &args).expect("recording");
        assert!(status.success());
        assert_eq!(
            invocation["verified_download_archives"],
            if available { json!([]) } else { Value::Null },
            "{name}"
        );
    }
}

#[test]
fn offline_project_commands_still_capture_archives_created_by_executed_code() {
    for (name, args) in [
        ("offline-build-archive", vec!["build", "--offline"]),
        ("offline-test-archive", vec!["test", "--offline"]),
        (
            "offline-nextest-archive",
            vec!["nextest", "run", "--offline"],
        ),
    ] {
        let (status, invocation) = recording(
            name,
            "explicit",
            "missing",
            "printf nested > \"$ARCHIVE\"",
            0,
            &args,
        )
        .expect("recording");
        assert!(status.success());
        assert_eq!(
            invocation["verified_download_archives"],
            json!(["registry/demo-0.1.0.crate"])
        );
    }
}

fn recording(
    name: &str,
    home: &str,
    initial: &str,
    action: &str,
    code: u8,
    args: &[&str],
) -> Result<(ExitStatus, Value), Box<dyn std::error::Error>> {
    let temp = std::env::temp_dir().join(format!("velnor-archives-{}-{name}", std::process::id()));
    let fixture = crate::run_capture::fixture(&temp, true, false)?;
    let program = if args.is_empty() {
        fixture.clone()
    } else {
        temp.join("cargo")
    };
    if program != fixture {
        fs::rename(fixture, &program)?;
    }
    let cargo_home = if home == "default" {
        temp.join("home/.cargo")
    } else {
        temp.join("cargo-home")
    };
    let cache = cargo_home.join("registry/cache");
    let archive = cache.join("registry/demo-0.1.0.crate");
    if initial == "archive" {
        fs::create_dir_all(archive.parent().ok_or("missing archive parent")?)?;
        fs::write(&archive, "old")?;
    } else if initial == "file" {
        fs::create_dir_all(cargo_home.join("registry"))?;
        fs::write(&cache, "not a directory")?;
    }
    let prepare = if matches!(initial, "file" | "after-file") {
        ":"
    } else {
        "mkdir -p \"$(dirname \"$ARCHIVE\")\""
    };
    fs::write(
        &program,
        format!("#!/bin/sh\n{prepare}\n{action}\ncat events\nexit {code}\n"),
    )?;
    let mut command = Command::new(env!("CARGO_BIN_EXE_velnor-ci"));
    command
        .args(["run", "--"])
        .arg(&program)
        .args(args)
        .current_dir(&temp)
        .env("RUNNER_TEMP", &temp)
        .env("ARCHIVE", &archive)
        .env("CACHE", &cache)
        .env("HOME", temp.join("home"))
        .env_remove("CARGO_HOME");
    match home {
        "explicit" => {
            command.env("CARGO_HOME", &cargo_home);
        }
        "relative" => {
            command.env("CARGO_HOME", "cargo-home");
        }
        _ => {}
    }
    let output = command.output()?;
    let text = fs::read_to_string(temp.join("velnor-invocations.jsonl"))?;
    let invocation = serde_json::from_str(text.trim())?;
    fs::remove_dir_all(temp)?;
    Ok((output.status, invocation))
}
