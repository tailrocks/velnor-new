use super::{
    DIR, FILE, attestation_fetch_script, candidate_path, manifest_attestation_bundle_script,
    manifest_script, tag_preflight_script,
};
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use velnor_actions_contract::RELEASE_MANIFEST_FILENAME;

const MANIFEST_PRODUCER: &str =
    include_str!("../../../scripts/generator-release/create-release-manifest.sh");

struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

fn scratch() -> Result<Scratch, Box<dyn Error>> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "velnor-release-preflight-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir(&directory)?;
    Ok(Scratch(directory))
}

#[test]
fn manifest_bundle_uses_the_created_and_attested_file() {
    let create = manifest_script("1.98.1", "1.21.1");
    let fetch = manifest_attestation_bundle_script();
    assert!(create.contains("create-release-manifest.sh"));
    assert!(
        fetch.contains("subject='manifest-assets/release-manifest.json'"),
        "{fetch}"
    );
    assert!(!fetch.contains(&format!("subject='{FILE}'")));
}

#[test]
fn renderer_uses_the_public_canonical_manifest_filename() {
    assert_eq!(FILE, RELEASE_MANIFEST_FILENAME);
    assert_eq!(
        candidate_path(),
        format!("{DIR}/{RELEASE_MANIFEST_FILENAME}")
    );
    assert!(
        MANIFEST_PRODUCER.contains(&format!("> {RELEASE_MANIFEST_FILENAME}")),
        "the shell producer must write the public canonical filename"
    );
}

#[test]
fn macos_attestation_fetch_uses_shasum_without_sha256sum() -> Result<(), Box<dyn Error>> {
    let scratch = scratch()?;
    let bin = scratch.0.join("bin");
    fs::create_dir(&bin)?;
    for command in ["awk", "shasum", "mv"] {
        symlink(find_command(command)?, bin.join(command))?;
    }
    let gh = bin.join("gh");
    fs::write(
        &gh,
        r#"#!/bin/bash
set -eu
case "$1 $2" in
  "attestation download")
    digest="$(shasum -a 256 "$3" | awk 'NR == 1 { print $1; next } { exit 1 } END { if (NR != 1) exit 1 }')"
    printf 'bundle for %s\n' "$3" > "sha256:${digest}.jsonl"
    ;;
  "attestation verify")
    printf '%s\n' "$*" > "$GH_VERIFY_LOG"
    ;;
  *) exit 71 ;;
esac
"#,
    )?;
    let mut permissions = fs::metadata(&gh)?.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&gh, permissions)?;
    assert!(!bin.join("sha256sum").exists());

    fs::write(scratch.0.join("candidate"), b"qualified candidate bytes")?;
    fs::create_dir(scratch.0.join("release-attestations"))?;
    let script = format!(
        "set -eu\n{}",
        attestation_fetch_script("candidate", "candidate")
    );
    let verify_log = scratch.0.join("verify.log");
    let output = Command::new("/bin/bash")
        .args(["-e", "-c", &script])
        .current_dir(&scratch.0)
        .env("PATH", &bin)
        .env("GITHUB_WORKFLOW_SHA", "a".repeat(40))
        .env("GITHUB_SHA", "a".repeat(40))
        .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
        .env("GH_VERIFY_LOG", &verify_log)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        scratch
            .0
            .join("release-attestations/candidate.intoto.jsonl")
            .is_file()
    );
    let verification = fs::read_to_string(verify_log)?;
    assert!(verification.contains("--signer-digest"), "{verification}");
    assert!(verification.contains(&"a".repeat(40)), "{verification}");
    Ok(())
}

fn find_command(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    for directory in std::env::split_paths(&std::env::var_os("PATH").ok_or("missing PATH")?) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return candidate.canonicalize().map_err(Into::into);
        }
    }
    Err(format!("cannot find test command {name}").into())
}

#[test]
fn tag_preflight_accepts_only_confirmed_not_found_responses() -> Result<(), Box<dyn Error>> {
    for (case, accepted) in [
        ("confirmed-404", true),
        ("forbidden", false),
        ("network", false),
        ("malformed", false),
        ("missing-status", false),
    ] {
        let scratch = scratch()?;
        let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()?;
        let gh = scratch.0.join("gh");
        fs::write(
            &gh,
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$GH_CALLS"
case "$GH_CASE" in
  confirmed-404) printf 'HTTP/2 404\r\ncontent-type: application/json\r\n\r\n{"message":"Not Found","status":"404"}\n'; exit 1 ;;
  forbidden) printf 'HTTP/2 403\r\n\r\n{"message":"Forbidden","status":"403"}\n'; exit 1 ;;
  network) exit 22 ;;
  malformed) printf 'not-an-http-response\n\nnot-json\n'; exit 1 ;;
  missing-status) printf 'HTTP/2 404\r\n\r\n{"message":"Not Found"}\n'; exit 1 ;;
esac
"#,
        )?;
        let mut permissions = fs::metadata(&gh)?.permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&gh, permissions)?;
        let mut paths = vec![scratch.0.clone()];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").ok_or("missing PATH")?,
        ));
        let path = std::env::join_paths(paths)?;
        let output = Command::new("bash")
            .arg("-c")
            .arg(tag_preflight_script())
            .current_dir(workspace)
            .env("PATH", path)
            .env("GH_CALLS", scratch.0.join("calls"))
            .env("GH_CASE", case)
            .env("GITHUB_REPOSITORY", "tailrocks/velnor-new")
            .output()?;
        let calls = fs::read_to_string(scratch.0.join("calls"))?;
        assert_eq!(
            output.status.success(),
            accepted,
            "case {case}; stderr: {}; calls: {calls}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            calls.lines().next(),
            Some("api --include repos/tailrocks/velnor-new/git/ref/tags/v0.1.1")
        );
        assert_eq!(calls.lines().count(), if accepted { 2 } else { 1 });
        if accepted {
            assert_eq!(
                calls.lines().nth(1),
                Some("api --include repos/tailrocks/velnor-new/releases/tags/v0.1.1")
            );
        }
    }
    Ok(())
}
