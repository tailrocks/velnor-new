// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde_json::Value;
use std::{error::Error, fs, io, os::unix::fs::PermissionsExt, process::Command};
use velnor_actions_workflow_renderer::render_schema2_workflows;

type TestResult = Result<(), Box<dyn Error>>;

fn steps<'a>(document: &'a Value, job: Option<&str>) -> Result<&'a [Value], Box<dyn Error>> {
    let value = match job {
        Some(id) => &document["jobs"][id]["steps"],
        None => &document["runs"]["steps"],
    };
    value
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| io::Error::other("missing generated steps").into())
}

fn check_boundary(steps: &[Value], optional: bool) -> TestResult {
    let fetch = steps
        .iter()
        .position(|step| step["name"] == "Fetch exact source artifact metadata")
        .ok_or("missing metadata fetch")?;
    let admission = steps
        .iter()
        .position(|step| {
            step["run"]
                .as_str()
                .is_some_and(|run| run.contains("stable-source-admission.py"))
        })
        .ok_or("missing admission")?;
    let download = steps
        .iter()
        .position(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|uses| uses.starts_with("actions/download-artifact@"))
        })
        .ok_or("missing source download")?;
    assert!(fetch < admission && admission < download);
    assert_eq!(
        steps
            .iter()
            .filter(|step| step["name"] == "Install pinned GitHub CLI")
            .count(),
        1
    );
    assert!(
        steps[..fetch]
            .iter()
            .any(|step| step["name"] == "Install pinned GitHub CLI")
    );
    assert_eq!(steps[fetch]["env"]["GH_TOKEN"], "${{ github.token }}");
    assert_eq!(
        steps[fetch]["env"]["SOURCES_METADATA"],
        steps[admission]["env"]["SOURCES_METADATA"]
    );
    assert_eq!(
        steps[fetch]["continue-on-error"].as_bool().unwrap_or(false),
        optional
    );
    for step in &steps[fetch..=download] {
        if step["run"]
            .as_str()
            .is_some_and(|run| run.contains("scripts/"))
        {
            assert!(step["env"].get("GH_TOKEN").is_none());
            assert!(step["env"].get("GITHUB_TOKEN").is_none());
        }
    }
    Ok(())
}

#[test]
fn source_metadata_authentication_stays_in_generated_fetch_steps() -> TestResult {
    let files = render_schema2_workflows(&super::request(true)?)?;
    let mut boundaries = 0;
    for file in files {
        if file.path == ".github/workflows/qualification.yml" {
            let document: Value = serde_saphyr::from_str(&file.bytes)?;
            for platform in ["linux", "macos", "intel"] {
                let id = format!("stable-sources-{platform}-warm");
                check_boundary(steps(&document, Some(&id))?, false)?;
                boundaries += 1;
            }
        } else if file
            .path
            .starts_with(".github/actions/generator-release-qualify-")
        {
            let document: Value = serde_saphyr::from_str(&file.bytes)?;
            check_boundary(steps(&document, None)?, true)?;
            boundaries += 1;
        }
    }
    assert_eq!(boundaries, 7);
    Ok(())
}

#[test]
fn source_metadata_fetch_rejects_invalid_ids_and_partial_or_oversized_responses() -> TestResult {
    let directory = tempfile::tempdir()?;
    let gh = directory.path().join("gh");
    fs::write(
        &gh,
        "#!/bin/bash\nset -eu\nprintf '%s\\n' \"$*\" > \"$TEST_CALLED\"\ntest \"$GH_TOKEN\" = canary\ncat \"$TEST_PAYLOAD\"\ntest \"$TEST_MODE\" != failed\n",
    )?;
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o700))?;
    let mut request = super::request(true)?;
    request
        .product_release
        .as_mut()
        .ok_or("missing release pins")?
        .gh_argv = vec![gh.to_string_lossy().into_owned()];
    let files = render_schema2_workflows(&request)?;
    let file = files
        .iter()
        .find(|file| file.path == ".github/workflows/qualification.yml")
        .ok_or("missing qualification workflow")?;
    let document: Value = serde_saphyr::from_str(&file.bytes)?;
    let fetch = steps(&document, Some("stable-sources-linux-warm"))?
        .iter()
        .find(|step| step["name"] == "Fetch exact source artifact metadata")
        .ok_or("missing fetch")?;
    let run = fetch["run"].as_str().ok_or("missing fetch script")?;
    let metadata = directory.path().join("metadata");
    let partial = directory.path().join("metadata.partial");
    let called = directory.path().join("called");
    let payload = directory.path().join("payload");
    for (id, mode, content, success, invoked) in [
        ("42", "valid", b"{\"id\":42}".to_vec(), true, true),
        ("0", "valid", Vec::new(), false, false),
        ("-1", "valid", Vec::new(), false, false),
        ("42?x=1", "valid", Vec::new(), false, false),
        ("42", "failed", b"partial".to_vec(), false, true),
        ("42", "valid", vec![b' '; 1_048_577], false, true),
    ] {
        fs::write(&metadata, "stale")?;
        fs::write(&partial, "stale partial")?;
        fs::write(&payload, &content)?;
        if called.exists() {
            fs::remove_file(&called)?;
        }
        let output = Command::new("bash")
            .args(["-c", run])
            .env("GH_TOKEN", "canary")
            .env("SOURCES_ID", id)
            .env("SOURCES_METADATA", &metadata)
            .env("TEST_CALLED", &called)
            .env("TEST_PAYLOAD", &payload)
            .env("TEST_MODE", mode)
            .output()?;
        assert_eq!(
            output.status.success(),
            success,
            "{id} {mode}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(called.exists(), invoked);
        assert!(!partial.exists());
        if success {
            assert_eq!(fs::read(&metadata)?, content);
        } else {
            assert!(!metadata.exists());
        }
        if invoked {
            assert_eq!(
                fs::read_to_string(&called)?,
                "api --hostname github.com --method GET repos/tailrocks/velnor-new/actions/artifacts/42\n"
            );
        }
    }
    Ok(())
}
