// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::io::{Read as _, Write as _};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[test]
fn native_revalidation_binds_real_cargo_archive_and_registry_state()
-> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("VELNOR_TEST_REVALIDATION_CHILD").is_some() {
        return child();
    }
    let temp = tempfile::tempdir()?;
    std::fs::create_dir(temp.path().join("src"))?;
    std::fs::write(
        temp.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.1'\nedition='2021'\nlicense='MIT'\ndescription='fixture'\n[features]\nexample=[]\n",
    )?;
    std::fs::write(temp.path().join("src/lib.rs"), "pub fn fixture() {}\n")?;
    run(temp.path(), "cargo", &["generate-lockfile", "--offline"])?;
    run(temp.path(), "git", &["init", "-q"])?;
    run(temp.path(), "git", &["add", "."])?;
    run(
        temp.path(),
        "git",
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "commit",
            "-qm",
            "fixture",
        ],
    )?;
    run(
        temp.path(),
        "cargo",
        &["package", "--locked", "--offline", "--target-dir", "target"],
    )?;
    let archive = std::fs::read(temp.path().join("target/package/demo-0.1.1.crate"))?;
    for scenario in [
        "valid", "archive", "checksum", "features", "owners", "yank", "index",
    ] {
        exercise_scenario(&archive, temp.path(), scenario)?;
    }
    Ok(())
}

fn run(
    root: &std::path::Path,
    program: &str,
    args: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let output = std::process::Command::new(program)
        .current_dir(root)
        .args(args)
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn child() -> Result<(), Box<dyn std::error::Error>> {
    let archive = std::fs::read(std::env::var("VELNOR_TEST_REVALIDATION_ARCHIVE")?)?;
    let scenario = std::env::var("VELNOR_TEST_REVALIDATION_CASE")?;
    let registry = NativeCrateRegistryRead;
    let owners = vec![CrateRegistryOwner {
        id: 1,
        login: "owner".into(),
        kind: "user".into(),
    }];
    let frozen = reconcile_crate_registry(&registry, "demo", "0.1.1", &owners, &archive)?;
    registry.get("/api/v1/crates/demo")?;
    let result = revalidate_crate_registry(&registry, "demo", "0.1.1", &frozen);
    if scenario == "valid" || scenario == "index" {
        assert_eq!(result?, frozen);
        let index = wait_for_crate_index(&registry, "demo", "0.1.1", &frozen.checksum, 1, || {});
        assert_eq!(index.is_ok(), scenario == "valid");
    } else {
        assert!(result.is_err(), "changed {scenario} was accepted");
    }
    Ok(())
}

fn exercise_scenario(
    archive: &[u8],
    root: &std::path::Path,
    scenario: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let origin = format!("http://{}", listener.local_addr()?);
    let stop = Arc::new(AtomicBool::new(false));
    let done = Arc::clone(&stop);
    let data = archive.to_vec();
    let mode = scenario.to_owned();
    let server = std::thread::spawn(move || serve(&listener, &done, &data, &mode));
    let output = std::process::Command::new(std::env::current_exe()?)
        .args([
            "--exact",
            crate::libtest_case::name(concat!(
                module_path!(),
                "::native_revalidation_binds_real_cargo_archive_and_registry_state"
            )),
        ])
        .env("VELNOR_TEST_REVALIDATION_CHILD", "1")
        .env("VELNOR_TEST_REVALIDATION_CASE", scenario)
        .env(
            "VELNOR_TEST_REVALIDATION_ARCHIVE",
            root.join("target/package/demo-0.1.1.crate"),
        )
        .env("VELNOR_TEST_REGISTRY_ORIGIN", origin)
        .output()?;
    stop.store(true, Ordering::Release);
    server
        .join()
        .map_err(|_| "server panicked")?
        .map_err(|error| error.to_string())?;
    assert!(
        output.status.success(),
        "{scenario}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    crate::libtest_case::assert_one_passed(&output);
    Ok(())
}

fn serve(
    listener: &std::net::TcpListener,
    stop: &AtomicBool,
    archive: &[u8],
    scenario: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let checksum = crate::planner::generator::sha256_hex(archive);
    let mut changed = false;
    while !stop.load(Ordering::Acquire) {
        let (mut stream, _) = match listener.accept() {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(std::time::Duration::from_millis(5));
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;
        let mut request = [0_u8; 8192];
        let length = stream.read(&mut request)?;
        let line = String::from_utf8_lossy(&request[..length]);
        let path = line
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .ok_or("bad request")?;
        let bytes = response(path, archive, &checksum, scenario, changed)?;
        if path == "/api/v1/crates/demo" {
            changed = true;
        }
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        )?;
        stream.write_all(&bytes)?;
    }
    Ok(())
}

fn response(
    path: &str,
    archive: &[u8],
    checksum: &str,
    scenario: &str,
    changed: bool,
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let changed = if changed { scenario } else { "valid" };
    let value = match path {
        "/api/v1/crates/demo" => serde_json::json!({"crate":{"id":"demo","name":"demo"}}),
        "/api/v1/crates/demo/owners" => {
            serde_json::json!({"users":[{"id":if changed == "owners" {2} else {1},"login":"owner","kind":"user"}]})
        }
        "/api/v1/crates/demo/0.1.1" => {
            serde_json::json!({"version":{"crate":"demo","num":"0.1.1","yanked":changed == "yank","checksum":if changed == "checksum" {"f".repeat(64)} else {checksum.to_owned()},"features":if changed == "features" {serde_json::json!({"example":["bogus"]})} else {serde_json::json!({"example":[]})}}})
        }
        "/crates/demo/demo-0.1.1.crate" => {
            let mut bytes = archive.to_vec();
            if changed == "archive" {
                bytes.push(0);
            }
            return Ok(bytes);
        }
        "/de/mo/demo" => {
            serde_json::json!({"name":"demo","vers":"0.1.1","cksum":if changed == "index" {"f".repeat(64)} else {checksum.to_owned()},"yanked":false})
        }
        _ => return Err(format!("unexpected path {path}").into()),
    };
    Ok(serde_json::to_vec(&value)?)
}
