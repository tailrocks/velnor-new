// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    collections::BTreeMap, error::Error, fs, os::unix::fs::PermissionsExt, path::Path,
    process::Command, time::Duration,
};

use std::fmt::Write as _;

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use velnor_actions_mise::{BoundedCommandStatus, BoundedCommandTermination, run_command_bounded};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const SOURCE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const INPUTS: [&str; 5] = [
    ".velnor/runner.lock",
    "images/job/ubuntu-26.04/Dockerfile",
    "images/job/ubuntu-26.04/README.md",
    "images/job/ubuntu-26.04/build-test.sh",
    "images/job/ubuntu-26.04/candidate/candidate.py",
];

struct RenderedStep {
    run: String,
    env: BTreeMap<String, String>,
    source: String,
}

fn rendered_step() -> Result<RenderedStep> {
    let text = crate::gc1_t51::candidate_text()?;
    let start = text
        .find("      - name: Fetch authenticated source inputs\n")
        .ok_or("source fetch step missing")?;
    let materializers = text[..start]
        .rsplit_once("name: Materialize fixed script")
        .ok_or("materializer missing")?
        .1;
    let runs = crate::bounded_test_support::materializer_runs(&format!(
        "name: Materialize fixed script{materializers}"
    ))?;
    let source = crate::bounded_test_support::materialized_source(&runs)?;
    let tail = &text[start + 1..];
    let end = tail.find("\n      - ").unwrap_or(tail.len());
    let section = &tail[..end];
    let encoded = section
        .lines()
        .find_map(|line| line.strip_prefix("        run: "))
        .ok_or("source fetch script missing")?;
    let run = serde_json::from_str(encoded)?;
    let mut lines = section.lines().skip_while(|line| line.trim() != "env:");
    lines.next().ok_or("source fetch environment missing")?;
    let mut env = BTreeMap::new();
    for line in lines {
        let Some(entry) = line.strip_prefix("          ") else {
            break;
        };
        let (key, value) = entry.split_once(": ").ok_or("invalid environment entry")?;
        let value = if value.starts_with('"') {
            serde_json::from_str(value)?
        } else {
            value.to_owned()
        };
        env.insert(key.to_owned(), value);
    }
    Ok(RenderedStep { run, env, source })
}

fn stage_external_api(root: &Path, malformed: bool) -> Result<Value> {
    let api = root.join("api");
    fs::create_dir(&api)?;
    let mut cases = String::from(
        "#!/bin/sh\nset -eu\n: > \"$SOURCE_API_FIXTURES/invoked\"\ntest \"$#\" -eq 2\ntest \"$1\" = api\ncase \"$2\" in\n",
    );
    let mut digests = serde_json::Map::new();
    for (index, path) in INPUTS.iter().enumerate() {
        let bytes = format!("source fixture bytes for {path}\n").into_bytes();
        let payload = json!({"type": if malformed { "dir" } else { "file" }, "encoding": "base64", "content": STANDARD.encode(&bytes)});
        fs::write(
            api.join(format!("{index}.json")),
            serde_json::to_vec(&payload)?,
        )?;
        let mut digest = String::with_capacity(64);
        for byte in Sha256::digest(&bytes) {
            write!(&mut digest, "{byte:02x}")?;
        }
        digests.insert((*path).to_owned(), json!(digest));
        writeln!(
            &mut cases,
            "'repos/fixture/source/contents/{path}?ref={SOURCE}') cat \"$SOURCE_API_FIXTURES/{index}.json\" ;;"
        )?;
    }
    cases.push_str("*) exit 89 ;;\nesac\n");
    let gh = root.join("gh");
    fs::write(&gh, cases)?;
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o700))?;
    Ok(Value::Object(digests))
}

fn execute_step(
    step: &RenderedStep,
    root: &Path,
    runner_temp: &Path,
    adapt_gh: bool,
) -> Result<velnor_actions_mise::BoundedCommandOutput> {
    let path = std::env::join_paths(std::iter::once(root.to_path_buf()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))?;
    let runner_temp_text = runner_temp.to_str().ok_or("non-UTF8 fixture path")?;
    let script_dir = root.join("fixed-script");
    fs::create_dir(&script_dir)?;
    let mut source = step.source.clone();
    let mut run = step.run.clone();
    if adapt_gh {
        let bootstrap = velnor_actions_workflow_renderer::verified_gh_shell::bootstrap();
        let gh = velnor_actions_workflow_renderer::verified_gh_shell::command();
        assert_eq!(source.matches(&bootstrap).count(), 1);
        let hex = |bytes: &[u8]| {
            bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        let original = hex(&Sha256::digest(source.as_bytes()));
        source = source.replace(&bootstrap, ":");
        assert_eq!(source.matches(&gh).count(), 2);
        source = source.replace(&gh, "command gh");
        let adapted = hex(&Sha256::digest(source.as_bytes()));
        assert_eq!(run.matches(&original).count(), 1);
        run = run.replace(&original, &adapted);
    }
    fs::write(script_dir.join("script.sh"), &source)?;
    let mut command = Command::new("/bin/bash");
    command
        .args(["-c", &run])
        .current_dir(root)
        .env_clear()
        .env("PATH", path)
        .env("RUNNER_TEMP", runner_temp)
        .env("VELNOR_FIXED_SCRIPT_DIR", script_dir)
        .env("SOURCE_API_FIXTURES", root.join("api"));
    for (key, value) in &step.env {
        let value = value
            .replace("${{ runner.temp }}", runner_temp_text)
            .replace("${{ github.repository }}", "fixture/source")
            .replace("${{ github.sha }}", SOURCE)
            .replace("${{ secrets.GITHUB_TOKEN }}", "");
        command.env(key, value);
    }
    let output = run_command_bounded(
        &mut command,
        1024 * 1024,
        Duration::from_secs(30),
        Duration::from_secs(10),
    );
    if output.termination != BoundedCommandTermination::Completed
        || output.execution_error.is_some()
        || output.cleanup_error.is_some()
    {
        return Err(format!("source fetch process did not settle: {output:?}").into());
    }
    Ok(output)
}

#[test]
fn rendered_source_fetch_writes_the_runtime_path_and_rejects_invalid_responses() -> Result<()> {
    let step = rendered_step()?;
    let fixture = tempfile::tempdir()?;
    let untrusted = fixture.path().join("untrusted");
    fs::create_dir(&untrusted)?;
    let untrusted_temp = untrusted.join("runner");
    fs::create_dir(&untrusted_temp)?;
    stage_external_api(&untrusted, false)?;
    let untrusted_output = execute_step(&step, &untrusted, &untrusted_temp, false)?;
    assert_ne!(untrusted_output.status, BoundedCommandStatus::Exited(0));
    assert!(
        String::from_utf8_lossy(&untrusted_output.stderr).contains("verified_gh_bootstrap_failed")
    );
    assert!(!untrusted.join("api/invoked").exists());
    assert!(
        !untrusted_temp
            .join("velnor-authenticated-source.json")
            .exists()
    );
    for malformed in [false, true] {
        let root = fixture
            .path()
            .join(if malformed { "malformed" } else { "valid" });
        fs::create_dir(&root)?;
        let runner_temp = root.join("runner temp with spaces");
        fs::create_dir(&runner_temp)?;
        let expected_inputs = stage_external_api(&root, malformed)?;
        let output = execute_step(&step, &root, &runner_temp, true)?;
        let result = runner_temp.join("velnor-authenticated-source.json");
        if malformed {
            assert_ne!(output.status, BoundedCommandStatus::Exited(0));
            assert!(!result.exists());
            assert!(String::from_utf8_lossy(&output.stderr).contains("source file is not a file:"));
        } else {
            assert_eq!(
                output.status,
                BoundedCommandStatus::Exited(0),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let actual: Value = serde_json::from_slice(&fs::read(result)?)?;
            assert_eq!(actual, json!({"commit": SOURCE, "inputs": expected_inputs}));
        }
    }
    Ok(())
}
