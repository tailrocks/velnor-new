// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::json;
use sha2::{Digest, Sha256};
use std::{error::Error, fs, path::Path, process::Command};

#[test]
fn qualification_rejects_mutated_checkout_lock_before_download() -> Result<(), Box<dyn Error>> {
    exercise("dirty")
}

#[test]
fn qualification_rejects_unbound_tool_plan_before_download() -> Result<(), Box<dyn Error>> {
    exercise("forged")
}

#[test]
fn qualification_accepts_committed_lock_before_download() -> Result<(), Box<dyn Error>> {
    exercise("valid")
}

fn exercise(case: &str) -> Result<(), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let checkout = root.join("checkout");
    let runner = root.join("runner");
    let target = root.join("target");
    fs::create_dir_all(&checkout)?;
    fs::create_dir_all(runner.join("source-qualification-artifacts/logs"))?;
    fs::create_dir_all(target.join("release"))?;
    git(&checkout, &["init", "--quiet"])?;
    let mut parsed: toml::Value = toml::from_str(include_str!("../../../../mise.lock"))?;
    for name in ["jactionlint", "shellcheck"] {
        parsed["tools"][name][0]["platforms.linux-x64"]["url"] =
            toml::Value::String("https://127.0.0.1:1/archive.tar.gz".into());
    }
    let lock = toml::to_string(&parsed)?.into_bytes();
    fs::write(checkout.join("mise.lock"), &lock)?;
    git(&checkout, &["add", "mise.lock"])?;
    git(&checkout, &["commit", "--quiet", "-m", "source lock"])?;
    let head = git(&checkout, &["rev-parse", "HEAD"])?;
    let binary = target.join("release/velnor-actions");
    fs::copy(env!("CARGO_BIN_EXE_velnor-actions"), &binary)?;
    let binary_sha = hex(&Sha256::digest(fs::read(&binary)?));
    let evidence = runner.join("source-qualification-artifacts");
    let version = concat!("velnor-actions ", env!("CARGO_PKG_VERSION"));
    fs::write(
        evidence.join("cli-identity.txt"),
        format!(
            "source_head={head}\nbinary_path={}\nexpected_version={version}\nbinary_version={version}\nbinary_sha256={binary_sha}\n",
            binary.display()
        ),
    )?;
    for name in [
        "validator-tool-install.exit",
        "validator-tool-lock-admission.exit",
    ] {
        fs::write(evidence.join("logs").join(name), "0\n")?;
    }
    let tools = ["jactionlint", "shellcheck"].into_iter().map(|name| {
        let tool = &parsed["tools"][name][0];
        let platform = &tool["platforms.linux-x64"];
        json!({"tool":name,"platform":"linux-x64","version":tool["version"].as_str(),"root_backend":tool["backend"].as_str(),"url":platform["url"].as_str(),"checksum":platform["checksum"].as_str()})
    }).collect::<Vec<_>>();
    let mut lock_sha = hex(&Sha256::digest(&lock));
    let mut plan = json!({"lock_sha256":lock_sha,"platform":"linux-x64","tools":tools});
    if case == "forged" {
        plan["tools"][0]["url"] = json!("https://127.0.0.1:2/archive.tar.gz");
    }
    if case == "dirty" {
        let dirty = [lock.as_slice(), b"\n# changed checkout\n"].concat();
        fs::write(checkout.join("mise.lock"), &dirty)?;
        lock_sha = hex(&Sha256::digest(&dirty));
        plan["lock_sha256"] = json!(lock_sha);
    }
    fs::write(
        evidence.join("validator-tool-lock-admission.txt"),
        format!("source_head={head}\nlock_sha256={lock_sha}\n"),
    )?;
    fs::write(
        evidence.join("validator-tool-install-plan.json"),
        serde_json::to_vec(&plan)?,
    )?;
    let output = Command::new(&binary)
        .current_dir(&checkout)
        .env_remove("HTTPS_PROXY")
        .env_remove("https_proxy")
        .env_remove("ALL_PROXY")
        .env_remove("all_proxy")
        .env("NO_PROXY", "*")
        .env("no_proxy", "*")
        .env("VELNOR_INTERNAL_OP", "qualify-validator-tools-v1")
        .env("GITHUB_EVENT_NAME", "workflow_dispatch")
        .env("GITHUB_RUN_ID", "123")
        .env("GITHUB_SHA", &head)
        .env("RUNNER_TEMP", &runner)
        .env("CARGO_TARGET_DIR", &target)
        .output()?;
    assert_outcome(&output, &evidence, case)
}

fn assert_outcome(
    output: &std::process::Output,
    evidence: &Path,
    case: &str,
) -> Result<(), Box<dyn Error>> {
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    let phase = evidence.join("native-validator-tool-qualification");
    if case == "valid" {
        assert!(
            phase
                .join("archives/jactionlint-linux-x64.tar.gz")
                .is_file(),
            "valid committed plan did not reach download"
        );
    } else {
        let expected = if case == "forged" {
            "native_validator_lock_tool_mismatch"
        } else {
            "native_validator_lock_admission_mismatch"
        };
        assert!(stderr.contains(expected), "expected {expected}: {stderr}");
        assert!(!phase.join("archives").exists());
    }
    assert!(!phase.join("candidate.json").exists());
    assert!(fs::read_to_string(phase.join("status.txt"))?.contains("result=failed"));
    Ok(())
}

fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git")
        .current_dir(root)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(format!("git failed: {}", String::from_utf8_lossy(&output.stderr)).into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(char::from(HEX[usize::from(byte >> 4)]));
        output.push(char::from(HEX[usize::from(byte & 15)]));
    }
    output
}
