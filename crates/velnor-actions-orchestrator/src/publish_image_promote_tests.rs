//! Behavioral cases: the fixed promote script runs for real against fake tools.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use tempfile::TempDir;
use velnor_actions_workflow_renderer::image_publish_scripts::promote_script;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn sh(script: &str, dir: &Path, env: &[(&str, &str)]) -> std::io::Result<Output> {
    Command::new("sh")
        .args(["-c", script])
        .current_dir(dir)
        .env_clear()
        .env(
            "PATH",
            "/usr/bin:/bin:/usr/local/bin:/opt/homebrew/bin:/sbin",
        )
        .envs(env.iter().copied())
        .output()
}

/// Run the promote script with fake `crane` and `gh`; returns (success, log).
fn promote(env: &[(&str, &str)]) -> Result<(bool, String), Box<dyn std::error::Error>> {
    let temp = TempDir::new()?;
    let log = temp.path().join("log");
    let crane = temp.path().join("crane");
    let gh = temp.path().join("gh");
    fs::write(
        &crane,
        format!(
            "#!/bin/sh\necho \"crane $@\" >> '{log}'\nstate='{log}.rel'\ncase \"$1\" in digest) case \"$2\" in *:sha-*) echo \"$SHA_DIGEST\" ;; *) if [ -f \"$state\" ]; then cat \"$state\"; elif [ -n \"$REL_DIGEST\" ]; then echo \"$REL_DIGEST\"; else exit 1; fi ;; esac ;; tag) echo \"${{2##*@}}\" > \"$state\" ;; esac\n",
            log = log.display()
        ),
    )?;
    fs::write(
        &gh,
        format!(
            "#!/bin/sh\necho \"gh $@\" >> '{log}'\n[ \"$GH_FAIL\" != 1 ]\n",
            log = log.display()
        ),
    )?;
    sh(
        &format!("chmod +x '{}' '{}'", crane.display(), gh.display()),
        temp.path(),
        &[],
    )?;
    let script = promote_script(
        &format!("crane() {{ '{}' \"$@\"; }}", crane.display()),
        &format!("gh() {{ '{}' \"$@\"; }}", gh.display()),
        "ghcr.io/acme/demo",
        "main",
    );
    let mut vars = vec![
        ("RUNNER_TEMP", temp.path().to_str().ok_or("path")?),
        ("GITHUB_SHA", "1111111111111111111111111111111111111111"),
        ("GITHUB_REF_NAME", "demo-v1.2.3"),
        ("GITHUB_REPOSITORY", "acme/widgets"),
    ];
    vars.extend_from_slice(env);
    let output = sh(&script, temp.path(), &vars)?;
    Ok((
        output.status.success(),
        fs::read_to_string(&log).unwrap_or_default(),
    ))
}

#[test]
fn promote_tags_only_a_verified_digest_and_is_idempotent() -> TestResult {
    let digest = format!("sha256:{}", "c".repeat(64));
    let base = [
        ("SHA_DIGEST", digest.as_str()),
        ("REL_DIGEST", ""),
        ("GH_FAIL", "0"),
    ];
    let (ok, log) = promote(&base)?;
    assert!(ok, "{log}");
    assert!(
        log.contains("--source-digest 1111111111111111111111111111111111111111"),
        "{log}"
    );
    assert!(log.contains("--source-ref refs/heads/main"), "{log}");
    assert!(
        log.contains(&format!("crane tag ghcr.io/acme/demo@{digest} 1.2.3")),
        "{log}"
    );
    let same = [
        ("SHA_DIGEST", digest.as_str()),
        ("REL_DIGEST", digest.as_str()),
        ("GH_FAIL", "0"),
    ];
    let (ok, log) = promote(&same)?;
    assert!(ok && !log.contains("crane tag"), "{log}");
    Ok(())
}

#[test]
fn promote_fails_without_attestation_or_on_a_moved_release_tag() -> TestResult {
    let digest = format!("sha256:{}", "c".repeat(64));
    let other = format!("sha256:{}", "d".repeat(64));
    let (ok, log) = promote(&[
        ("SHA_DIGEST", &digest),
        ("REL_DIGEST", ""),
        ("GH_FAIL", "1"),
    ])?;
    assert!(
        !ok && !log.contains("crane tag"),
        "unattested digest: {log}"
    );
    let (ok, log) = promote(&[
        ("SHA_DIGEST", &digest),
        ("REL_DIGEST", &other),
        ("GH_FAIL", "0"),
    ])?;
    assert!(
        !ok && !log.contains("crane tag"),
        "release tag names another digest: {log}"
    );
    let (ok, _) = promote(&[
        ("SHA_DIGEST", "latest"),
        ("REL_DIGEST", ""),
        ("GH_FAIL", "0"),
    ])?;
    assert!(!ok, "a non-digest value is refused");
    Ok(())
}
