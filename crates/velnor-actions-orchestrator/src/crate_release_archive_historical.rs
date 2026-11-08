// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{OrchestratorError, internal, read_archive};

pub(crate) fn verify_historical_archive_content(
    current: &[u8],
    published: &[u8],
    package: &str,
    version: &str,
    checksum: &str,
    current_sha: &str,
    historical_sha: &str,
) -> Result<(), OrchestratorError> {
    let invalid = || internal("crate_archive_historical_content_mismatch");
    let valid_sha = |s: &str| {
        s.len() == 40
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    };
    if !valid_sha(current_sha)
        || !valid_sha(historical_sha)
        || current_sha == historical_sha
        || checksum.len() != 64
        || !checksum
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || crate::planner::generator::sha256_hex(published) != checksum
    {
        return Err(invalid());
    }
    let prefix = format!("{package}-{version}/");
    let mut current = read_archive(current, &prefix)?;
    let mut historical = read_archive(published, &prefix)?;
    let (current_mode, current_vcs) = current.remove(".cargo_vcs_info.json").ok_or_else(invalid)?;
    let (historical_mode, historical_vcs) = historical
        .remove(".cargo_vcs_info.json")
        .ok_or_else(invalid)?;
    if current_mode != historical_mode || current != historical {
        return Err(invalid());
    }
    let parse = |bytes: &[u8], sha: &str| -> Result<String, OrchestratorError> {
        let value =
            velnor_actions_contract::strict_json::parse_strict_json_bytes(bytes, 1024 * 1024)
                .map_err(|_| invalid())?;
        if value.as_object().is_none_or(|v| v.len() != 2)
            || value["git"].as_object().is_none_or(|v| v.len() != 1)
            || value["git"]["sha1"].as_str() != Some(sha)
        {
            return Err(invalid());
        }
        value["path_in_vcs"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(invalid)
    };
    if parse(&current_vcs, current_sha)? != parse(&historical_vcs, historical_sha)? {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod historical_archive_tests {
    use super::*;
    use std::process::Command;

    fn command(
        dir: &std::path::Path,
        program: &str,
        args: &[&str],
    ) -> Result<String, Box<dyn std::error::Error>> {
        let output = Command::new(program).args(args).current_dir(dir).output()?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
        }
        Ok(String::from_utf8(output.stdout)?.trim().to_owned())
    }

    fn commit(
        root: &std::path::Path,
        path: &str,
        name: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        command(root, "git", &["add", path])?;
        command(
            root,
            "git",
            &[
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-qm",
                name,
            ],
        )?;
        command(root, "git", &["rev-parse", "HEAD"])
    }

    fn archive(root: &std::path::Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        command(
            root,
            "cargo",
            &[
                "package",
                "--offline",
                "--locked",
                "--no-verify",
                "--target-dir",
                "target",
                "-p",
                "historytest",
            ],
        )?;
        Ok(std::fs::read(
            root.join("target/package/historytest-0.1.1.crate"),
        )?)
    }

    #[test]
    fn real_cargo_history_requires_equal_payload_checksum_and_commit()
    -> Result<(), Box<dyn std::error::Error>> {
        let temp = tempfile::tempdir()?;
        let root = temp.path();
        std::fs::create_dir_all(root.join("demo/src"))?;
        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nresolver='2'\nmembers=['demo']\n",
        )?;
        std::fs::write(
            root.join("demo/Cargo.toml"),
            "[package]\nname='historytest'\nversion='0.1.1'\nedition='2021'\n",
        )?;
        std::fs::write(root.join("demo/src/lib.rs"), "pub fn value() -> u8 { 1 }\n")?;
        command(root, "git", &["init", "-q"])?;
        command(root, "cargo", &["generate-lockfile", "--offline"])?;
        let old_sha = commit(root, ".", "first")?;
        let old = archive(root)?;
        std::fs::write(root.join("unrelated"), "reviewed unrelated change")?;
        let new_sha = commit(root, "unrelated", "second")?;
        let current = archive(root)?;
        let checksum = crate::planner::generator::sha256_hex(&old);
        let verify = |bytes: &[u8], hash: &str, current_sha: &str, historical_sha: &str| {
            verify_historical_archive_content(
                bytes,
                &old,
                "historytest",
                "0.1.1",
                hash,
                current_sha,
                historical_sha,
            )
        };
        assert!(verify(&current, &checksum, &new_sha, &old_sha).is_ok());
        assert!(verify(&current, &checksum, &new_sha, &"f".repeat(40)).is_err());
        assert!(verify(&current, &"0".repeat(64), &new_sha, &old_sha).is_err());
        std::fs::write(root.join("demo/src/lib.rs"), "pub fn value() -> u8 { 2 }\n")?;
        let changed_sha = commit(root, "demo/src/lib.rs", "changed")?;
        assert!(verify(&archive(root)?, &checksum, &changed_sha, &old_sha).is_err());
        Ok(())
    }
}
