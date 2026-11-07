// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn real_native_qualification() -> Result<(), Box<dyn std::error::Error>> {
    use std::{env, fs, path::PathBuf, process::Command};
    use velnor_actions_contract::{
        parse_image_candidate_record, parse_native_image_qualification_record,
    };

    if env::var("GITHUB_ACTIONS").as_deref() != Ok("true")
        || env::var("GITHUB_REPOSITORY").as_deref() != Ok("tailrocks/velnor-new")
    {
        return Ok(());
    }
    let event = env::var("GITHUB_EVENT_NAME")?;
    let source_ref = env::var("GITHUB_REF")?;
    if event != "pull_request"
        && !(matches!(event.as_str(), "push" | "workflow_dispatch")
            && source_ref == "refs/heads/main")
    {
        return Ok(());
    }
    let root = PathBuf::from(env::var_os("GITHUB_WORKSPACE").ok_or("missing workspace")?);
    let temp = PathBuf::from(env::var_os("RUNNER_TEMP").ok_or("missing runner temp")?);
    let binary = env!("CARGO_BIN_EXE_velnor-actions");
    let invoke = || {
        let mut command = Command::new(binary);
        command
            .args(["image-maintenance", "run-native-qualification", "--root"])
            .arg(&root)
            .arg("--runner-temp")
            .arg(&temp);
        command
    };
    let rejected = invoke().env("GITHUB_SHA", "invalid-source").output()?;
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("native_qualification_source_identity")
    );

    let output = invoke()
        .env("RUSTFLAGS", "--velnor-native-environment-canary")
        .output()?;
    assert!(
        output.status.success(),
        "qualification failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let fixture_path = temp
        .join("velnor-native-image-qualification/target/release/velnor-native-image-qualification");
    let fixture = Command::new(&fixture_path).output()?;
    assert!(fixture.status.success());
    assert_eq!(
        String::from_utf8(fixture.stdout)?.trim(),
        "velnor-native-image-qualification:547"
    );
    let bytes = fs::read(temp.join("native-image-qualification.json"))?;
    let record = parse_native_image_qualification_record(&bytes)?;
    assert_eq!(record.source_commit, env::var("GITHUB_SHA")?);
    assert_eq!(record.source_ref, env::var("GITHUB_REF")?);
    assert_eq!(
        record.workflow.run_id.to_string(),
        env::var("GITHUB_RUN_ID")?
    );
    assert_eq!(
        record.workflow.run_attempt.to_string(),
        env::var("GITHUB_RUN_ATTEMPT")?
    );
    assert!(parse_image_candidate_record(&bytes).is_err());
    assert_eq!(hash_file(&fixture_path)?, record.binary_sha256);
    for (path, expected) in &record.source_inputs {
        assert_eq!(&hash_file(&root.join(path))?, expected);
    }
    check_oci(&temp, &record)?;
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn hash_file(path: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(hex(&hash.finalize()))
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(char::from(DIGITS[usize::from(byte >> 4)]));
        value.push(char::from(DIGITS[usize::from(byte & 15)]));
    }
    value
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn check_oci(
    temp: &std::path::Path,
    record: &velnor_actions_contract::NativeImageQualificationRecord,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::{fs, process::Command};
    let archive = temp.join("native-image-qualification.oci.tar");
    assert_eq!(fs::metadata(&archive)?.len(), record.archive.size_bytes);
    assert_eq!(hash_file(&archive)?, record.archive.sha256);
    check_config_blob(&archive, record)?;
    let config = temp.join(".velnor-job-image-candidate/docker-config");
    let load = Command::new("/usr/bin/docker")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .arg("--config")
        .arg(&config)
        .args(["image", "load", "--input"])
        .arg(&archive)
        .output()?;
    assert!(
        load.status.success(),
        "docker load: {}",
        String::from_utf8_lossy(&load.stderr)
    );
    let run = Command::new("/usr/bin/docker")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .arg("--config")
        .arg(&config)
        .args([
            "run",
            "--rm",
            "--network",
            "none",
            "--read-only",
            "--platform",
            "linux/amd64",
        ])
        .arg(&record.oci.image_configuration_digest)
        .output()?;
    assert!(
        run.status.success(),
        "docker run: {}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(
        String::from_utf8(run.stdout)?.trim(),
        "velnor-native-image-qualification:547"
    );
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn check_config_blob(
    archive: &std::path::Path,
    record: &velnor_actions_contract::NativeImageQualificationRecord,
) -> Result<(), Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let digest = record
        .oci
        .image_configuration_digest
        .strip_prefix("sha256:")
        .ok_or("config digest")?;
    let expected = format!("blobs/sha256/{digest}");
    let mut tar = tar::Archive::new(std::fs::File::open(archive)?);
    let mut found = false;
    for entry in tar.entries()? {
        let mut entry = entry?;
        if entry.path()?.as_ref() != std::path::Path::new(&expected) {
            continue;
        }
        assert!(!found);
        assert_eq!(entry.size(), record.oci.image_configuration_size_bytes);
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        assert_eq!(hex(&Sha256::digest(&bytes)), digest);
        let config: serde_json::Value = serde_json::from_slice(&bytes)?;
        assert_eq!(config["config"]["User"], "65532:65532");
        assert_eq!(
            config["config"]["Entrypoint"],
            serde_json::json!(["/fixture"])
        );
        let cmd = &config["config"]["Cmd"];
        assert!(cmd.is_null() || cmd.as_array().is_some_and(Vec::is_empty));
        found = true;
    }
    assert!(found);
    Ok(())
}
