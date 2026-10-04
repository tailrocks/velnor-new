use super::action_snapshots::{Actions, action};
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

#[derive(Clone, Copy)]
struct TargetJobs {
    target: &'static str,
    build: &'static str,
    qualify: &'static str,
    attest: &'static str,
    qualifier_action: &'static str,
    attester_action: &'static str,
}

pub(super) fn assert_isolated_candidate_execution(
    workflow: &str,
    actions: &Actions,
) -> Result<(), Box<dyn Error>> {
    for target in [
        TargetJobs {
            target: "linux",
            build: "build-linux",
            qualify: "qualify-linux",
            attest: "attest-linux",
            qualifier_action: "generator-release-qualify-linux",
            attester_action: "generator-release-attest-linux",
        },
        TargetJobs {
            target: "macos",
            build: "build-macos",
            qualify: "qualify-macos",
            attest: "attest-macos",
            qualifier_action: "generator-release-qualify-macos",
            attester_action: "generator-release-attest-macos",
        },
        TargetJobs {
            target: "macos-intel",
            build: "build-macos-intel",
            qualify: "qualify-macos-intel",
            attest: "attest-macos-intel",
            qualifier_action: "generator-release-qualify-macos-intel",
            attester_action: "generator-release-attest-macos-intel",
        },
    ] {
        assert_target_boundary(workflow, actions, target)?;
    }
    candidate_environment_writes_stay_in_qualifier()?;
    Ok(())
}

fn assert_target_boundary(
    workflow: &str,
    actions: &Actions,
    target: TargetJobs,
) -> Result<(), Box<dyn Error>> {
    assert_build_artifact_outputs(workflow, target.build)?;
    assert_qualifier_boundary(workflow, actions, target)?;
    assert_attester_boundary(workflow, actions, target)
}

fn assert_build_artifact_outputs(workflow: &str, build: &str) -> Result<(), Box<dyn Error>> {
    let build_job = super::super::job_body(workflow, build)?;
    assert!(
        build_job.contains("outputs:\n      artifact_id:"),
        "{build_job}"
    );
    assert!(build_job.contains("artifact_digest:"), "{build_job}");
    assert!(build_job.contains("id: upload\n"), "{build_job}");
    assert!(build_job.contains("artifact-id"), "{build_job}");
    assert!(build_job.contains("artifact-digest"), "{build_job}");
    Ok(())
}

fn assert_qualifier_boundary(
    workflow: &str,
    actions: &Actions,
    target: TargetJobs,
) -> Result<(), Box<dyn Error>> {
    let qualifier = super::super::job_body(workflow, target.qualify)?;
    assert!(qualifier.contains("contents: read"), "{qualifier}");
    for permission in [
        "actions: write",
        "contents: write",
        "id-token: write",
        "attestations: write",
    ] {
        assert!(
            !qualifier.contains(permission),
            "{}: {qualifier}",
            target.target
        );
    }
    assert!(
        qualifier.contains(&format!("- {}\n", target.build)),
        "{qualifier}"
    );
    assert!(!qualifier.contains("outputs:"), "{qualifier}");
    assert!(
        qualifier.contains("persist-credentials: \"false\""),
        "{qualifier}"
    );
    assert_job_action(workflow, target.qualify, target.qualifier_action)?;
    let qualifier_body = action(actions, target.qualifier_action)?;
    let artifact_id = artifact_id_input(target.build);
    assert!(qualifier_body.contains(&artifact_id), "{qualifier_body}");
    assert!(
        qualifier_body.contains("cache: \"false\""),
        "{qualifier_body}"
    );
    assert!(
        qualifier_body.contains("Qualify downloaded candidate"),
        "{qualifier_body}"
    );
    assert_eq!(
        qualifier_body
            .matches("Qualify downloaded candidate")
            .count(),
        1
    );
    assert!(!qualifier_body.contains("GH_TOKEN:"), "{qualifier_body}");
    assert!(
        !qualifier_body.contains("Attest built artifacts"),
        "{qualifier_body}"
    );
    assert!(
        !qualifier_body.contains("actions/upload-artifact@"),
        "{qualifier_body}"
    );
    let candidate = qualifier_body
        .find("Qualify downloaded candidate")
        .ok_or("missing candidate execution")?;
    assert!(
        !qualifier_body[candidate..].contains("- name:"),
        "{qualifier_body}"
    );
    Ok(())
}

fn assert_attester_boundary(
    workflow: &str,
    actions: &Actions,
    target: TargetJobs,
) -> Result<(), Box<dyn Error>> {
    let attester = super::super::job_body(workflow, target.attest)?;
    assert!(
        attester.contains(&format!("- {}\n      - {}\n", target.build, target.qualify)),
        "{attester}"
    );
    assert!(attester.contains("id-token: write"), "{attester}");
    assert_job_action(workflow, target.attest, target.attester_action)?;
    let attester_body = action(actions, target.attester_action)?;
    let artifact_id = artifact_id_input(target.build);
    assert!(attester_body.contains(&artifact_id), "{attester_body}");
    assert!(
        attester_body.contains("Verify candidate provenance record"),
        "{attester_body}"
    );
    assert!(
        attester_body.contains("Verify downloaded checksum sidecar"),
        "{attester_body}"
    );
    assert!(
        attester_body.contains("Attest built artifacts"),
        "{attester_body}"
    );
    assert!(
        attester_body.contains("Fetch and verify candidate attestation bundles"),
        "{attester_body}"
    );
    assert!(
        attester_body.contains("Upload verified candidate attestation bundles"),
        "{attester_body}"
    );
    assert!(
        !attester_body.contains("Qualify downloaded candidate"),
        "{attester_body}"
    );
    assert!(!attester_body.contains("--version"), "{attester_body}");
    assert!(
        !attester_body.contains("capture-opentofu-goldens.sh"),
        "{attester_body}"
    );
    assert!(
        !attester_body.contains(&format!("needs.{}.outputs", target.qualify)),
        "{attester_body}"
    );
    Ok(())
}

fn artifact_id_input(build: &str) -> String {
    format!("artifact-ids: ${{{{ needs.{build}.outputs.artifact_id }}}}")
}

fn assert_job_action(workflow: &str, id: &str, action_name: &str) -> Result<(), Box<dyn Error>> {
    let job = super::super::job_body(workflow, id)?;
    assert!(
        job.contains(&format!("uses: ./.github/actions/{action_name}")),
        "{job}"
    );
    Ok(())
}

fn candidate_environment_writes_stay_in_qualifier() -> Result<(), Box<dyn Error>> {
    let qualifier = tempfile::TempDir::new()?;
    let attester = tempfile::TempDir::new()?;
    let qualifier_bin = qualifier.path().join("bin");
    let attester_bin = attester.path().join("bin");
    fs::create_dir_all(&qualifier_bin)?;
    fs::create_dir_all(&attester_bin)?;
    let malicious_gh = qualifier_bin.join("gh");
    write_executable(
        &malicious_gh,
        "#!/bin/sh\nprintf '%s\\n' leaked > \"$QUALIFIER_TOKEN_CAPTURE\"\n",
    )?;
    let candidate = qualifier.path().join("candidate.sh");
    write_executable(
        &candidate,
        "#!/bin/sh\nprintf '%s\\n' \"$QUALIFIER_BIN\" >> \"$GITHUB_PATH\"\nprintf '%s\\n' 'INJECTED=1' >> \"$GITHUB_ENV\"\n(sleep 0.05; : > \"$QUALIFIER_BACKGROUND\") >/dev/null 2>&1 &\n",
    )?;
    let qualifier_path = qualifier.path().join("github-path");
    let qualifier_env = qualifier.path().join("github-env");
    let background = qualifier.path().join("background-finished");
    fs::write(&qualifier_path, "")?;
    fs::write(&qualifier_env, "")?;
    let candidate_status = Command::new(&candidate)
        .env("QUALIFIER_BIN", &qualifier_bin)
        .env("GITHUB_PATH", &qualifier_path)
        .env("GITHUB_ENV", &qualifier_env)
        .env("QUALIFIER_BACKGROUND", &background)
        .status()?;
    assert!(candidate_status.success());

    let trusted_gh = attester_bin.join("gh");
    write_executable(
        &trusted_gh,
        "#!/bin/sh\nprintf '%s\\n' \"$GH_TOKEN\" > \"$ATTEST_TOKEN_CAPTURE\"\n",
    )?;
    let attest_path = attester.path().join("github-path");
    let attest_env = attester.path().join("github-env");
    let token_capture = attester.path().join("token-capture");
    fs::write(&attest_path, "")?;
    fs::write(&attest_env, "")?;
    let gh_status = Command::new("gh")
        .arg("api")
        .env("PATH", &attester_bin)
        .env("GITHUB_PATH", &attest_path)
        .env("GITHUB_ENV", &attest_env)
        .env("GH_TOKEN", "attest-job-token")
        .env("ATTEST_TOKEN_CAPTURE", &token_capture)
        .status()?;
    assert!(gh_status.success());
    assert_eq!(fs::read_to_string(&token_capture)?, "attest-job-token\n");
    assert!(!qualifier.path().join("token-capture").exists());
    assert_eq!(fs::read_to_string(&qualifier_env)?, "INJECTED=1\n");
    assert!(
        fs::read_to_string(&qualifier_path)?
            .contains(qualifier_bin.to_str().ok_or("non-UTF8 path")?)
    );
    assert_eq!(fs::read_to_string(&attest_path)?, "");
    assert_eq!(fs::read_to_string(&attest_env)?, "");
    for _ in 0..50 {
        if background.exists() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        background.exists(),
        "candidate background process did not run"
    );
    assert_eq!(fs::read_to_string(&attest_path)?, "");
    assert_eq!(fs::read_to_string(&attest_env)?, "");
    Ok(())
}

fn write_executable(path: &Path, content: &str) -> Result<(), Box<dyn Error>> {
    fs::write(path, content)?;
    let mut permissions = fs::metadata(path)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions)?;
    Ok(())
}
