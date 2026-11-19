// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, UNIX_EPOCH};

use crate::io::Repo;

fn command(repo: &Repo, directory: &Path, run: &str, trace: &Path) -> String {
    let binary = format!("'{}'", env!("CARGO_BIN_EXE_velnor-ci"));
    let run = match run.strip_prefix(". \"$VELNOR_EXEC\"") {
        Some(rest) => format!("{binary}{rest}"),
        None => run.replacen("velnor-ci", &binary, 1),
    };
    let output = Command::new("sh")
        .args(["-c", &run])
        .current_dir(directory)
        .env("GITHUB_WORKSPACE", repo.root())
        .env("RUNNER_TEMP", repo.root().join("job-temp"))
        .env("GITHUB_JOB", "rust-job")
        .env("GIT_TRACE2_EVENT", trace)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{run}: {text}");
    text
}

fn generated_steps(workflow: &str, name: &str) -> Vec<(String, String)> {
    workflow
        .split("\n      - ")
        .skip(1)
        .filter(|step| {
            step.lines()
                .any(|line| line.trim() == format!("name: {name}"))
        })
        .map(|step| {
            let value = |key: &str| {
                step.lines()
                    .find_map(|line| line.trim().strip_prefix(key))
                    .unwrap_or_default()
            };
            (
                value("working-directory: ").to_owned(),
                value("run: ").trim_matches('\'').replace("''", "'"),
            )
        })
        .collect()
}

fn cargo_build(repo: &Repo, root: &str) -> String {
    let output = Command::new(env!("CARGO"))
        .args(["build", "--offline", "-v"])
        .current_dir(repo.root().join(root))
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTC_WRAPPER")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "{root}: {text}");
    text
}

pub(crate) fn fixture() -> Repo {
    let repo = Repo::new("batch-custom-targets");
    repo.write(
        "mise.toml",
        "[tools]\nrust='1.99.1'\n'aqua:nextest-rs/nextest/cargo-nextest'='0.9.151'\n",
    );
    repo.write(
        "mise.lock",
        include_str!("../../../../generator/samples/lab/view/mise.lock"),
    );
    let lock = include_str!("../../../../generator/samples/lab/view/.velnor/rust-ci.lock")
        .replace("0.1.93", "0.1.97");
    repo.write(".velnor/rust-ci.lock", &lock);
    for (root, package, target) in [
        ("", "app", "custom-target"),
        ("one/", "one", "../one-target"),
        ("two/", "two", "../two-target"),
    ] {
        repo.write(
            &format!("{root}Cargo.toml"),
            &format!("[package]\nname='{package}'\nversion='0.1.0'\nedition='2024'\n"),
        );
        repo.write(&format!("{root}src/lib.rs"), "pub fn value() {}\n");
        repo.write(
            &format!("{root}.cargo/config.toml"),
            &format!("[build]\ntarget-dir='{target}'\n"),
        );
        let status = Command::new(env!("CARGO"))
            .args(["generate-lockfile", "--offline"])
            .current_dir(repo.root().join(root))
            .status()
            .unwrap_or_else(|e| unreachable!("{e}"));
        assert!(status.success());
    }
    repo.git(&["add", "--all"]);
    velnor_gen::io::write_metadata(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    repo.commit();
    repo
}

#[test]
#[ignore = "fails on the CI cluster only, tracked in #1820"]
fn generated_save_and_restore_share_custom_targets_and_detect_once_for_three_roots() {
    let repo = fixture();
    let checkout =
        velnor_gen::io::read_recorded_checkout(repo.root()).unwrap_or_else(|e| unreachable!("{e}"));
    let files = velnor_gen::generated_files(&checkout.view).unwrap_or_else(|e| unreachable!("{e}"));
    let workflow = files
        .iter()
        .find(|(path, _)| path.as_str().ends_with("rust-ci.yml"))
        .map_or_else(|| unreachable!(), |(_, text)| text);
    let built = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    for path in ["src/lib.rs", "one/src/lib.rs", "two/src/lib.rs"] {
        repo.set_mtime(path, built);
    }
    for root in ["", "one", "two"] {
        cargo_build(&repo, root);
    }
    let trace = repo.root().join("git-trace.jsonl");
    for (directory, run) in generated_steps(workflow, "velnor-ci inputs save") {
        let bootstrap = run.split(" --scope-cache ").next().unwrap_or_default();
        command(&repo, &repo.root().join(directory), bootstrap, &trace);
    }
    for target in ["custom-target", "one-target", "two-target"] {
        assert!(
            repo.root().join(target).join(".velnor").is_dir(),
            "{target}"
        );
    }
    let checkout_time = UNIX_EPOCH + Duration::from_hours(500_000);
    for path in ["src/lib.rs", "one/src/lib.rs", "two/src/lib.rs"] {
        repo.set_mtime(path, checkout_time);
    }
    fs::write(&trace, "").unwrap_or_else(|e| unreachable!("{e}"));
    let restores = generated_steps(workflow, "velnor-ci inputs restore");
    let (_, batch) = restores
        .iter()
        .find(|(_, run)| run.matches("--root-target").count() == 3)
        .unwrap_or_else(|| unreachable!("no three-root batch: {restores:?}"));
    command(&repo, repo.root(), batch, &trace);
    for path in ["src/lib.rs", "one/src/lib.rs", "two/src/lib.rs"] {
        assert_eq!(repo.mtime(path), built, "{path}");
    }
    command(
        &repo,
        repo.root(),
        "velnor-ci restored --target-dir custom-target --target-dir one-target --target-dir two-target",
        &trace,
    );
    for (directory, run) in generated_steps(workflow, "velnor-ci inputs save") {
        command(&repo, &repo.root().join(directory), &run, &trace);
    }
    let trace = fs::read_to_string(trace).unwrap_or_else(|e| unreachable!("{e}"));
    let detections = trace
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|event| {
            event["event"] == "start"
                && event["argv"].as_array().is_some_and(|args| {
                    args.len() == 4
                        && args[1..]
                            == [
                                serde_json::json!("config"),
                                serde_json::json!("--get"),
                                serde_json::json!("remote.origin.url"),
                            ]
                })
        })
        .count();
    assert_eq!(
        detections, 1,
        "checkout detection invocations for three roots"
    );
    for (root, package) in [("", "app"), ("one", "one"), ("two", "two")] {
        let text = cargo_build(&repo, root);
        assert!(text.contains(&format!("Fresh {package} ")), "{text}");
        assert!(!text.contains("Compiling "), "{text}");
    }
}
