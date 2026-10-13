use super::super::attestation_fetch_script;
use super::{find_command, scratch};
use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::fs::symlink;
use std::process::Command;

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
