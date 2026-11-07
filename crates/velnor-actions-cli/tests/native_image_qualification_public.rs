// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "native_image_qualification_public/service.rs"]
mod service;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn real_native_qualification() -> Result<(), Box<dyn std::error::Error>> {
    use std::{env, fs, path::PathBuf, process::Command};
    use velnor_actions_contract::parse_native_image_qualification_record;

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
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
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
    let rejected = bounded(invoke().env("GITHUB_SHA", "invalid-source"), deadline)?;
    assert_ne!(
        rejected.status,
        velnor_actions_mise::BoundedCommandStatus::Exited(0)
    );
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("native_qualification_source_identity")
    );

    if deadline.saturating_duration_since(std::time::Instant::now())
        < service::CLEANUP_RESERVE + service::START_RESERVE
    {
        return Err("insufficient qualification startup and cleanup time".into());
    }
    let daemon = service::Lease::start(&root, &temp, deadline)?;
    reject_tampered_service(binary, &root, &temp, deadline)?;
    let output = bounded(
        invoke().env("RUSTFLAGS", "--velnor-native-environment-canary"),
        deadline,
    )?;
    assert!(
        output.status == velnor_actions_mise::BoundedCommandStatus::Exited(0),
        "qualification failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let fixture_path = temp
        .join("velnor-native-image-qualification/target/release/velnor-native-image-qualification");
    let fixture = bounded(&mut Command::new(&fixture_path), deadline)?;
    assert_eq!(
        fixture.status,
        velnor_actions_mise::BoundedCommandStatus::Exited(0)
    );
    assert_eq!(
        String::from_utf8(fixture.stdout)?.trim(),
        "velnor-native-image-qualification:547"
    );
    let record_path = temp.join("native-image-qualification.json");
    let metadata = fs::metadata(&record_path)?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err("invalid qualification record".into());
    }
    let bytes = fs::read(record_path)?;
    let record = parse_native_image_qualification_record(&bytes)?;
    check_record(&record, &bytes, &fixture_path, &root, deadline)?;
    daemon.verify()?;
    check_oci(&temp, daemon.container_id()?, &record, deadline)?;
    daemon.verify()?;
    daemon.close()?;
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn check_record(
    record: &velnor_actions_contract::NativeImageQualificationRecord,
    bytes: &[u8],
    fixture_path: &std::path::Path,
    root: &std::path::Path,
    deadline: std::time::Instant,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::env;
    use velnor_actions_contract::parse_image_candidate_record;
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
    assert!(parse_image_candidate_record(bytes).is_err());
    assert_eq!(hash_file(fixture_path, deadline)?, record.binary_sha256);
    for (path, expected) in &record.source_inputs {
        assert_eq!(&hash_file(&root.join(path), deadline)?, expected);
    }
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn reject_tampered_service(
    binary: &str,
    root: &std::path::Path,
    temp: &std::path::Path,
    deadline: std::time::Instant,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        fs,
        io::{Read, Seek, SeekFrom, Write},
        process::Command,
    };
    let descriptor_path = temp.join("velnor-qualification-service.json");
    let original = fs::read(&descriptor_path)?;
    let mut descriptor: serde_json::Value = serde_json::from_slice(&original)?;
    descriptor["daemon_id"] = serde_json::json!("invalid-daemon-id");
    fs::write(&descriptor_path, serde_json::to_vec(&descriptor)?)?;
    let rejected = bounded(
        Command::new(binary)
            .args(["image-maintenance", "run-native-qualification", "--root"])
            .arg(root)
            .arg("--runner-temp")
            .arg(temp),
        deadline,
    );
    fs::write(&descriptor_path, &original)?;
    let rejected = rejected?;
    assert_ne!(
        rejected.status,
        velnor_actions_mise::BoundedCommandStatus::Exited(0)
    );
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("native_service_daemon_identity"));

    let archive = temp.join("velnor-qualification-service/docker.tgz");
    let mut file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(archive)?;
    let mut first = [0];
    file.read_exact(&mut first)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&[first[0] ^ 1])?;
    file.flush()?;
    let rejected = bounded(
        Command::new(binary)
            .args(["image-maintenance", "run-native-qualification", "--root"])
            .arg(root)
            .arg("--runner-temp")
            .arg(temp),
        deadline,
    );
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&first)?;
    file.flush()?;
    let rejected = rejected?;
    assert_ne!(
        rejected.status,
        velnor_actions_mise::BoundedCommandStatus::Exited(0)
    );
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("native_service_archive_identity"));
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn hash_file(
    path: &std::path::Path,
    deadline: std::time::Instant,
) -> Result<String, Box<dyn std::error::Error>> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > 512 * 1024 * 1024 {
        return Err("invalid qualification file".into());
    }
    let mut hash = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0; 8192];
    loop {
        if std::time::Instant::now()
            >= deadline
                .checked_sub(service::CLEANUP_RESERVE)
                .ok_or("qualification deadline")?
        {
            return Err("qualification deadline".into());
        }
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(count as u64)
            .filter(|bytes| *bytes <= 512 * 1024 * 1024)
            .ok_or("qualification file too large")?;
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
    container_id: &str,
    record: &velnor_actions_contract::NativeImageQualificationRecord,
    deadline: std::time::Instant,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;
    let archive = temp.join("native-image-qualification.oci.tar");
    assert_eq!(fs::metadata(&archive)?.len(), record.archive.size_bytes);
    assert_eq!(hash_file(&archive, deadline)?, record.archive.sha256);
    check_config_blob(&archive, record, deadline)?;
    let config = temp.join(".velnor-job-image-candidate/docker-config");
    let mut load = service::private_docker(container_id)?;
    load.arg("--config")
        .arg(&config)
        .args(["image", "load", "--input"])
        .arg(&archive);
    run_docker(&mut load, deadline)?;
    let mut run = service::private_docker(container_id)?;
    run.arg("--config")
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
        .arg(&record.oci.platform_manifest_digest);
    assert_eq!(
        run_docker(&mut run, deadline)?.trim(),
        "velnor-native-image-qualification:547"
    );
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn run_docker(
    command: &mut std::process::Command,
    deadline: std::time::Instant,
) -> Result<String, Box<dyn std::error::Error>> {
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", "/nonexistent");
    let output = bounded(command, deadline)?;
    if output.status != velnor_actions_mise::BoundedCommandStatus::Exited(0) {
        return Err(format!(
            "docker command failed: {:?}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn bounded(
    command: &mut std::process::Command,
    deadline: std::time::Instant,
) -> Result<velnor_actions_mise::BoundedCommandOutput, Box<dyn std::error::Error>> {
    use std::time::Duration;
    let remaining = deadline
        .checked_duration_since(std::time::Instant::now())
        .and_then(|value| value.checked_sub(service::CLEANUP_RESERVE))
        .ok_or("qualification deadline")?;
    let output = velnor_actions_mise::run_command_bounded(
        command,
        1024 * 1024,
        remaining.min(Duration::from_secs(600)),
        Duration::from_secs(5),
    );
    if output.termination != velnor_actions_mise::BoundedCommandTermination::Completed {
        return Err("qualification command did not complete".into());
    }
    Ok(output)
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn check_config_blob(
    archive: &std::path::Path,
    record: &velnor_actions_contract::NativeImageQualificationRecord,
    deadline: std::time::Instant,
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
        if std::time::Instant::now()
            >= deadline
                .checked_sub(service::CLEANUP_RESERVE)
                .ok_or("qualification deadline")?
        {
            return Err("qualification deadline".into());
        }
        let mut entry = entry?;
        if entry.path()?.as_ref() != std::path::Path::new(&expected) {
            continue;
        }
        assert!(!found);
        assert_eq!(entry.size(), record.oci.image_configuration_size_bytes);
        if entry.size() > 1024 * 1024 {
            return Err("qualification config too large".into());
        }
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
