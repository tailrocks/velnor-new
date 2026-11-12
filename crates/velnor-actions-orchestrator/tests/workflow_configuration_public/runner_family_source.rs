// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use sha2::{Digest, Sha256};
use std::{error::Error, fs, path::Path, process::Command};

#[test]
fn family_script_rejects_wrong_source_before_build_side_effects() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let root = directory.path().join("source");
    let scratch = directory.path().join("scratch");
    fs::create_dir(&root)?;
    fs::create_dir(&scratch)?;
    let original =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/check-runner-image-candidate.sh");
    let script = fs::read(&original)?;
    let text = std::str::from_utf8(&script)?;
    assert!(
        text.contains(
            "image-maintenance build-runner-images --root \"$root\" --source-commit \"$source_sha\""
        ),
        "producer invocation lost the selected source"
    );
    assert!(
        text.contains("VELNOR_SOURCE_SHA=\"$source_sha\" cargo build"),
        "helper build lost the selected source"
    );
    assert!(
        text.contains("check-runner-image-candidate \"$root\" \"$source_sha\""),
        "qualifier lost the selected source"
    );
    fs::write(root.join("candidate.sh"), &script)?;
    git(&root, &["init", "--quiet"])?;
    git(&root, &["config", "user.name", "Alexey Zhokhov"])?;
    git(&root, &["config", "user.email", "alexey@zhokhov.com"])?;
    git(&root, &["config", "commit.gpgsign", "false"])?;
    git(&root, &["add", "candidate.sh"])?;
    git(
        &root,
        &[
            "commit",
            "-s",
            "--quiet",
            "-m",
            "fixture\n\nCo-authored-by: Codex <codex@openai.com>",
        ],
    )?;
    let head = git(&root, &["rev-parse", "HEAD"])?;
    let wrong = if head.trim() == "a".repeat(40) {
        "b".repeat(40)
    } else {
        "a".repeat(40)
    };
    let output = Command::new("/bin/bash")
        .arg(root.join("candidate.sh"))
        .current_dir(&root)
        .env("GITHUB_SHA", wrong)
        .env("RUNNER_TEMP", &scratch)
        .env(
            "VELNOR_EXPECTED_RUSTUP_TOOLCHAIN",
            "1.99.1-x86_64-unknown-linux-gnu",
        )
        .output()?;
    assert!(
        !output.status.success(),
        "wrong source reached the family build"
    );
    assert_eq!(
        fs::read_dir(&scratch)?.count(),
        0,
        "wrong source created build workspace"
    );
    assert_eq!(fs::read(root.join("candidate.sh"))?, script);
    Ok(())
}
fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn Error>> {
    let output = Command::new("git").current_dir(root).args(args).output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string().into());
    }
    Ok(String::from_utf8(output.stdout)?)
}
pub(super) fn digest(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::new();
    for byte in Sha256::digest(bytes) {
        result.push(char::from(DIGITS[usize::from(byte >> 4)]));
        result.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    result
}
