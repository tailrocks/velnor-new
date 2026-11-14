// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn family_files()
-> Result<Vec<velnor_actions_workflow_renderer::RenderedFile>, Box<dyn Error>> {
    let mut request = request()?;
    request.runner_image_family = Some(
        velnor_actions_contract::config::MacRunnerQualificationConfig {
            runner_id: 2,
            companion_workflow_path: ".github/workflows/qualify-runner-family.yml".to_owned(),
            companion_workflow_ref: "refs/heads/main".to_owned(),
            companion_workflow_sha: "c".repeat(40),
            docker_socket: "unix:///tmp/fixture.sock".to_owned(),
            state_dir: "/tmp/fixture-state".to_owned(),
            sdk: velnor_actions_contract::config::MacSdkSelection::Stable {
                version: "1.2.3".to_owned(),
                source_sha: "a".repeat(40),
                manifest_sha256: "b".repeat(64),
            },
        },
    );
    Ok(render_schema2_workflows(&request)?)
}

#[test]
fn runner_qualification_requires_exact_receipt_and_source_free_signer() -> Result<(), Box<dyn Error>>
{
    let files = family_files()?;
    let caller = find_file(&files, ".github/workflows/runner-image-qualification.yml")?;
    let signer = find_file(
        &files,
        ".github/workflows/runner-image-qualification-attest.yml",
    )?;
    let caller_header = caller
        .split("jobs:")
        .next()
        .ok_or("qualification caller header missing")?;
    assert!(caller_header.contains("runner-image-qualification-caller-${{ github.ref }}"));
    assert!(caller_header.contains("cancel-in-progress: true"));
    assert!(!caller_header.contains("${{ github.repository }}-product-release"));
    let publisher_header = find_file(&files, ".github/workflows/product-release-images.yml")?
        .split("jobs:")
        .next()
        .ok_or("runner image release header missing")?;
    assert!(publisher_header.contains("${{ github.repository }}-product-release"));
    assert!(publisher_header.contains("cancel-in-progress: false"));
    let discovery = job_section(caller, "  discover-producer:", Some("  qualify-images:"))?;
    assert!(discovery.contains("timeout-minutes: 360"));
    assert!(!discovery.contains("packages: write"));
    let release = find_file(&files, ".github/workflows/product-release-images.yml")?;
    let release_producer = job_section(
        release,
        "  discover-producer:",
        Some("  discover-artifacts:"),
    )?;
    assert!(release_producer.contains("timeout-minutes: 360"));
    assert!(!release_producer.contains("packages: write"));
    let release_discovery =
        job_section(release, "  discover-artifacts:", Some("  publish-images:"))?;
    assert!(release_discovery.contains("timeout-minutes: 360"));
    assert!(!release_discovery.contains("packages: write"));
    assert!(release_discovery.contains("discover-producer"));
    let publisher = job_section(release, "  publish-images:", Some("  verify-mac-proof:"))?;
    assert!(publisher.contains("discover-artifacts"));
    assert!(publisher.contains("packages: write"));
    let qualifier = job_section(caller, "  qualify-images:", Some("  attest-qualification:"))?;
    assert!(qualifier.contains("timeout-minutes: 120"));
    assert!(qualifier.contains("discover-producer"));
    let attestor = job_section(caller, "  attest-qualification:", None)?;
    assert!(qualifier.contains("resolve-runner-family-producer.sh"));
    assert!(qualifier.contains("qualify-runner-image-archives"));
    assert!(qualifier.contains("needs.discover-producer.outputs.producer_artifact_id"));
    assert!(qualifier.contains("needs.discover-producer.outputs.producer_run_id"));
    assert!(qualifier.contains("steps.identity.outputs.qualifier_job_id"));
    assert!(qualifier.contains("runner-image-publication.json"));
    assert!(qualifier.contains("dind-image-publication.json"));
    assert!(qualifier.contains("resource-probe-image-publication.json"));
    for name in [
        "runner-image-provenance.json",
        "dind-image-provenance.json",
        "resource-probe-image-provenance.json",
    ] {
        assert!(qualifier.contains(name), "qualifier omits {name}");
    }
    assert!(qualifier.contains(r#"$assets\" \"$assets/runner-image-publication.json"#));
    assert!(!qualifier.contains("id-token: write"));
    assert!(!qualifier.contains("attestations: write"));
    assert!(attestor.contains("qualify-images"));
    assert!(attestor.contains("needs.qualify-images.outputs.receipt_artifact_id"));
    assert!(attestor.contains("runner-image-qualification-attest.yml"));
    assert!(signer.contains("inputs.receipt_artifact_id"));
    assert!(signer.contains("RUNNER_IMAGE_QUALIFICATION.json"));
    assert!(signer.contains(ATTEST_BUILD_PROVENANCE_USES));
    assert!(!signer.contains(CHECKOUT_USES));
    Ok(())
}

#[cfg(unix)]
#[test]
fn runner_qualification_discovers_unique_actual_run_artifact_and_job() -> Result<(), Box<dyn Error>>
{
    use std::{fs, os::unix::fs::PermissionsExt, process::Command};

    let dir = tempfile::tempdir()?;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let gh = dir.path().join("gh");
    fs::write(
        &gh,
        "#!/bin/sh\ncase \"$*\" in *workflows/ci.yml/runs*) cat \"$FIXTURE/runs\";; *actions/runs/31/artifacts*) cat \"$FIXTURE/artifacts\";; *actions/runs/41/attempts/2/jobs*) cat \"$FIXTURE/jobs\";; *) exit 1;; esac\n",
    )?;
    fs::set_permissions(&gh, fs::Permissions::from_mode(0o700))?;
    let sha = "a".repeat(40);
    fs::write(
        dir.path().join("runs"),
        format!(
            r#"[{{"total_count":1,"workflow_runs":[{{"path":".github/workflows/ci.yml","event":"push","head_branch":"main","head_sha":"{sha}","head_repository":{{"full_name":"tailrocks/velnor-new"}},"id":31,"run_number":7,"run_attempt":2,"status":"completed","conclusion":"success"}}]}}]"#
        ),
    )?;
    fs::write(
        dir.path().join("artifacts"),
        format!(
            r#"[{{"total_count":1,"artifacts":[{{"id":53,"name":"image-assets-2","expired":false,"size_in_bytes":10,"workflow_run":{{"id":31,"head_sha":"{sha}","head_branch":"main"}}}}]}}]"#
        ),
    )?;
    let job = format!(
        r#"{{"id":71,"name":"qualify-images","status":"in_progress","runner_name":"runner-1","head_sha":"{sha}","run_id":41,"run_attempt":2}}"#
    );
    fs::write(
        dir.path().join("jobs"),
        format!(r#"[{{"total_count":1,"jobs":[{job}]}}]"#),
    )?;
    let output = dir.path().join("output");
    let path = format!("{}:{}", dir.path().display(), std::env::var("PATH")?);
    let run = |mode: &str| -> std::io::Result<std::process::Output> {
        Command::new("/bin/bash")
            .arg(root.join("scripts/resolve-runner-family-producer.sh"))
            .args(if mode.is_empty() { vec![] } else { vec![mode] })
            .env("PATH", &path)
            .env("FIXTURE", dir.path())
            .env("GH_TOKEN", "fixture")
            .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
            .env("GITHUB_SHA", &sha)
            .env("GITHUB_RUN_ID", "41")
            .env("GITHUB_RUN_ATTEMPT", "2")
            .env("RUNNER_NAME", "runner-1")
            .env("GITHUB_OUTPUT", &output)
            .output()
    };
    let positive = run("")?;
    assert!(
        positive.status.success(),
        "valid producer identity rejected: {}",
        String::from_utf8_lossy(&positive.stderr)
    );
    assert_eq!(
        fs::read_to_string(&output)?,
        "producer_run_id=31\nproducer_run_attempt=2\nproducer_artifact_id=53\n"
    );
    fs::write(&output, "")?;
    assert!(run("qualifier")?.status.success());
    assert_eq!(fs::read_to_string(&output)?, "qualifier_job_id=71\n");
    assert_invalid_discovery(dir.path(), &output, &job, &run)?;
    Ok(())
}

#[cfg(unix)]
fn assert_invalid_discovery(
    dir: &std::path::Path,
    output: &std::path::Path,
    job: &str,
    run: &impl Fn(&str) -> std::io::Result<std::process::Output>,
) -> Result<(), Box<dyn Error>> {
    use std::fs;
    fs::write(output, "")?;
    fs::write(
        dir.join("jobs"),
        format!(r#"[{{"total_count":2,"jobs":[{job},{job}]}}]"#),
    )?;
    let duplicate = run("qualifier")?;
    assert!(
        !duplicate.status.success(),
        "ambiguous qualifier job accepted"
    );
    assert_eq!(fs::read_to_string(output)?, "");
    fs::write(
        dir.join("jobs"),
        format!(r#"[{{"total_count":1,"jobs":[{job}]}}]"#),
    )?;
    let artifact_path = dir.join("artifacts");
    let original_artifacts = fs::read(&artifact_path)?;
    let mut truncated: serde_json::Value = serde_json::from_slice(&original_artifacts)?;
    truncated[0]["total_count"] = serde_json::json!(2);
    fs::write(&artifact_path, serde_json::to_vec(&truncated)?)?;
    assert!(
        !run("")?.status.success(),
        "truncated artifact inventory accepted"
    );
    assert_eq!(fs::read_to_string(output)?, "");
    fs::write(&artifact_path, original_artifacts)?;
    let mut capped: serde_json::Value = serde_json::from_slice(&fs::read(dir.join("runs"))?)?;
    capped[0]["total_count"] = serde_json::json!(1000);
    let runs = capped[0]["workflow_runs"]
        .as_array_mut()
        .ok_or("runs missing")?;
    runs.extend((0..999).map(|id| serde_json::json!({"id": id + 100})));
    fs::write(dir.join("runs"), serde_json::to_vec(&capped)?)?;
    assert!(
        !run("")?.status.success(),
        "truncated workflow inventory accepted"
    );
    assert_eq!(fs::read_to_string(output)?, "");
    Ok(())
}

#[cfg(unix)]
#[test]
fn runner_qualification_waits_for_slow_ci_but_stops_at_its_bound() -> Result<(), Box<dyn Error>> {
    use std::{fs, process::Command};
    let dir = tempfile::tempdir()?;
    let sha = "a".repeat(40);
    let run = format!(
        r#"{{"path":".github/workflows/ci.yml","event":"push","head_branch":"main","head_sha":"{sha}","head_repository":{{"full_name":"tailrocks/velnor-new"}},"id":31,"run_number":7,"run_attempt":2,"status":"completed","conclusion":"success"}}"#
    );
    fs::write(
        dir.path().join("complete"),
        format!(r#"[{{"total_count":1,"workflow_runs":[{run}]}}]"#),
    )?;
    fs::write(
        dir.path().join("pending"),
        format!(
            r#"[{{"total_count":1,"workflow_runs":[{}]}}]"#,
            run.replace("\"completed\"", "\"in_progress\"")
        ),
    )?;
    fs::write(
        dir.path().join("artifacts"),
        format!(
            r#"[{{"total_count":1,"artifacts":[{{"id":53,"name":"image-assets-2","expired":false,"size_in_bytes":10,"workflow_run":{{"id":31,"head_sha":"{sha}","head_branch":"main"}}}}]}}]"#
        ),
    )?;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = r#"sleep() { SECONDS=$((SECONDS + 1201)); }
gh() { case "$*" in *workflows/ci.yml/runs*) if (( SECONDS > 1200 )) && [[ "$WAIT_FOREVER" != 1 ]]; then cat "$FIXTURE/complete"; else cat "$FIXTURE/pending"; fi ;; *actions/runs/31/artifacts*) cat "$FIXTURE/artifacts";; *) return 1;; esac; }
source "$RESOLVER"
"#;
    let output = dir.path().join("output");
    for (forever, success) in [("0", true), ("1", false)] {
        fs::write(&output, "")?;
        let result = Command::new("bash")
            .arg("-c")
            .arg(script)
            .env("FIXTURE", dir.path())
            .env(
                "RESOLVER",
                root.join("scripts/resolve-runner-family-producer.sh"),
            )
            .env("WAIT_FOREVER", forever)
            .env("GH_TOKEN", "fixture")
            .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
            .env("GITHUB_SHA", &sha)
            .env("GITHUB_RUN_ID", "41")
            .env("GITHUB_RUN_ATTEMPT", "2")
            .env("RUNNER_NAME", "runner-1")
            .env("GITHUB_OUTPUT", &output)
            .output()?;
        assert_eq!(
            result.status.success(),
            success,
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let emitted = fs::read_to_string(&output)?;
        if success {
            assert_eq!(
                emitted,
                "producer_run_id=31\nproducer_run_attempt=2\nproducer_artifact_id=53\n"
            );
        } else {
            assert_eq!(emitted, "");
        }
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn release_discovery_waits_for_each_late_phase_independently() -> Result<(), Box<dyn Error>> {
    use std::{fs, process::Command};
    let dir = tempfile::tempdir()?;
    let sha = "a".repeat(40);
    for (name, workflow, id, attempt) in [
        ("producer", "ci.yml", 31, 2),
        ("qualification", "runner-image-qualification.yml", 41, 3),
    ] {
        let run = format!(
            r#"{{"path":".github/workflows/{workflow}","event":"push","head_branch":"main","head_sha":"{sha}","head_repository":{{"full_name":"tailrocks/velnor-new"}},"id":{id},"run_number":7,"run_attempt":{attempt},"status":"completed","conclusion":"success"}}"#
        );
        fs::write(
            dir.path().join(format!("{name}-complete")),
            format!(r#"[{{"total_count":1,"workflow_runs":[{run}]}}]"#),
        )?;
        fs::write(
            dir.path().join(format!("{name}-pending")),
            format!(
                r#"[{{"total_count":1,"workflow_runs":[{}]}}]"#,
                run.replace("\"completed\"", "\"in_progress\"")
            ),
        )?;
    }
    fs::write(
        dir.path().join("producer-artifact"),
        format!(
            r#"[{{"total_count":1,"artifacts":[{{"id":53,"name":"image-assets-2","expired":false,"size_in_bytes":10,"workflow_run":{{"id":31,"head_sha":"{sha}","head_branch":"main"}}}}]}}]"#
        ),
    )?;
    fs::write(
        dir.path().join("qualification-artifact"),
        format!(
            r#"[{{"total_count":1,"artifacts":[{{"id":54,"name":"runner-image-qualification-3","expired":false,"size_in_bytes":10,"workflow_run":{{"id":41,"head_sha":"{sha}","head_branch":"main"}}}}]}}]"#
        ),
    )?;
    fs::write(
        dir.path().join("jobs"),
        format!(
            r#"[{{"total_count":1,"jobs":[{{"name":"qualify-images","status":"completed","conclusion":"success","head_sha":"{sha}","run_id":41,"run_attempt":3,"id":71}}]}}]"#
        ),
    )?;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let script = r#"sleep() { SECONDS=$((SECONDS + 1201)); }
gh() { case "$*" in *git/ref/heads/main*) printf '{"ref":"refs/heads/main","object":{"type":"commit","sha":"%s"}}\n' "$GITHUB_SHA";; *workflows/*/runs*) if (( SECONDS > WAIT_UNTIL )) && [[ "$WAIT_FOREVER" != 1 ]]; then cat "$FIXTURE/$VELNOR_FAMILY_DISCOVERY_PHASE-complete"; else cat "$FIXTURE/$VELNOR_FAMILY_DISCOVERY_PHASE-pending"; fi;; *runs/31/artifacts*) cat "$FIXTURE/producer-artifact";; *runs/41/artifacts*) cat "$FIXTURE/qualification-artifact";; *runs/41/attempts/3/jobs*) cat "$FIXTURE/jobs";; *) return 1;; esac; }
source "$RESOLVER"
"#;
    for (phase, forever, expected) in [
        (
            "producer",
            "0",
            "producer_run_id=31\nproducer_run_attempt=2\nproducer_artifact_id=53\n",
        ),
        (
            "qualification",
            "0",
            "qualifier_run_id=41\nqualifier_run_attempt=3\nqualifier_job_id=71\nqualification_artifact_id=54\n",
        ),
        ("producer", "1", ""),
        ("qualification", "1", ""),
    ] {
        let output = dir.path().join("output");
        fs::write(&output, "")?;
        let result = Command::new("bash")
            .arg("-c")
            .arg(script)
            .env("FIXTURE", dir.path())
            .env(
                "RESOLVER",
                root.join("scripts/resolve-runner-family-publication.sh"),
            )
            .env("WAIT_FOREVER", forever)
            .env(
                "WAIT_UNTIL",
                if phase == "producer" { "19800" } else { "6600" },
            )
            .env("GH_TOKEN", "fixture")
            .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
            .env("GITHUB_SHA", &sha)
            .env("GITHUB_OUTPUT", &output)
            .env("VELNOR_FAMILY_DISCOVERY_PHASE", phase)
            .env("VELNOR_FAMILY_DISCOVERY_TIMEOUT_SECONDS", "21000")
            .output()?;
        assert_eq!(
            result.status.success(),
            forever == "0",
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(fs::read_to_string(output)?, expected);
    }
    let aggregate = r#"sleep() { SECONDS=$((SECONDS + 1201)); }
gh() { case "$*" in *workflows/ci.yml/runs*) if (( SECONDS > 19800 )); then cat "$FIXTURE/producer-complete"; else cat "$FIXTURE/producer-pending"; fi;; *workflows/runner-image-qualification.yml/runs*) if (( SECONDS > 26400 )); then cat "$FIXTURE/qualification-complete"; else cat "$FIXTURE/qualification-pending"; fi;; *) return 1;; esac; }
source "$DISCOVERY"
deadline=$((SECONDS + 21000))
family_run ci.yml "$deadline" >/dev/null
family_run runner-image-qualification.yml "$deadline" >/dev/null
"#;
    let old = Command::new("bash")
        .arg("-e")
        .arg("-c")
        .arg(aggregate)
        .env("FIXTURE", dir.path())
        .env("DISCOVERY", root.join("scripts/runner-family-discovery.sh"))
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env("GITHUB_SHA", &sha)
        .output()?;
    assert!(
        !old.status.success(),
        "aggregate deadline should expire before qualification"
    );
    Ok(())
}
