// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#![allow(
    missing_docs,
    reason = "documentation comments are intentionally absent from this codebase"
)]
use std::env;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::process::{self, Command, Stdio};
use std::time::{Duration, Instant};

const SOURCE_ENV: &str = "VELNOR_RUNNER_BUILD_SOURCE_SHA";
const HELPER_ENV: &str = "VELNOR_RUNNER_BUILD_HELPER_SHA256";
const COMPILED_SOURCE_ENV: &str = "VELNOR_COMPILED_SOURCE_SHA";
const COMPILED_HELPER_ENV: &str = "VELNOR_COMPILED_ATTESTATION_HELPER_SHA256";

fn main() {
    println!("cargo:rerun-if-env-changed={SOURCE_ENV}");
    println!("cargo:rerun-if-env-changed={HELPER_ENV}");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/compile_identity.rs");
    if let Err(error) = configure() {
        eprintln!("runner release identity: {error}");
        process::exit(1);
    }
}

fn configure() -> Result<(), String> {
    let source = read_input(SOURCE_ENV)?;
    let helper = read_input(HELPER_ENV)?;
    match (source, helper) {
        (None, None) => write_unavailable(),
        (Some(source), Some(helper)) => write_available(&source, &helper),
        _ => Err("source and helper digests must be supplied together".to_owned()),
    }
}

fn read_input(name: &str) -> Result<Option<String>, String> {
    env::var_os(name).map_or(Ok(None), |value| {
        value
            .into_string()
            .map(Some)
            .map_err(|_| format!("{name} must be UTF-8 lowercase hexadecimal"))
    })
}

fn write_available(source: &str, helper: &str) -> Result<(), String> {
    let source_bytes = parse_hex::<20>(SOURCE_ENV, source)?;
    let runner_lock = verify_committed_runner_lock(source)?;
    write_runner_lock(&runner_lock)?;
    let helper_bytes = parse_hex::<32>(HELPER_ENV, helper)?;
    emit_test_values(source, helper);
    let generated = format!(
        "const COMPILED_RELEASE_IDENTITY: Option<CompiledReleaseIdentity> = Some(\n\
         CompiledReleaseIdentity {{ source_sha: {}, helper_sha256: {} }});\n",
        array_literal(&source_bytes),
        array_literal(&helper_bytes),
    );
    write_generated(&generated)
}

fn verify_committed_runner_lock(source: &str) -> Result<Vec<u8>, String> {
    let manifest = env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| "missing manifest directory".to_owned())?;
    let root = manifest.join("../../../..");
    let object = format!("{source}:.velnor/runner.lock");
    let mut command = Command::new("/usr/bin/git");
    command
        .arg("-C")
        .arg(root)
        .args(["cat-file", "blob", &object]);
    for name in [
        "GIT_DIR",
        "GIT_EXEC_PATH",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
        "GIT_COMMON_DIR",
        "GIT_NAMESPACE",
    ] {
        command.env_remove(name);
    }
    command
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_ALLOW_PROTOCOL", "")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|_| "cannot inspect committed runner lock".to_owned())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "cannot read git output".to_owned())?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(64 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|_| "cannot wait for git".to_owned())?
        {
            break status;
        }
        if Instant::now() >= deadline {
            let killed = child.kill();
            let reaped = child.wait();
            let _reader = reader.join();
            if killed.is_err() || reaped.is_err() {
                return Err("cannot stop git".to_owned());
            }
            return Err("git timed out".to_owned());
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let committed = reader
        .join()
        .map_err(|_| "cannot read git output".to_owned())?
        .map_err(|_| "cannot read git output".to_owned())?;
    if !status.success() || committed.is_empty() || committed.len() > 64 * 1024 {
        return Err("runner lock does not match source commit".to_owned());
    }
    Ok(committed)
}

fn write_unavailable() -> Result<(), String> {
    write_runner_lock(&[])?;
    println!("cargo:rustc-env={COMPILED_SOURCE_ENV}=unavailable");
    println!("cargo:rustc-env={COMPILED_HELPER_ENV}=unavailable");
    write_generated("const COMPILED_RELEASE_IDENTITY: Option<CompiledReleaseIdentity> = None;\n")
}

fn emit_test_values(source: &str, helper: &str) {
    println!("cargo:rustc-env={COMPILED_SOURCE_ENV}={source}");
    println!("cargo:rustc-env={COMPILED_HELPER_ENV}={helper}");
}

fn parse_hex<const LENGTH: usize>(name: &str, value: &str) -> Result<[u8; LENGTH], String> {
    if value.len() != LENGTH * 2 {
        return Err(format!(
            "{name} must contain exactly {} lowercase hexadecimal characters",
            LENGTH * 2
        ));
    }
    let mut decoded = [0_u8; LENGTH];
    for (index, byte) in decoded.iter_mut().enumerate() {
        let offset = index * 2;
        let high = value
            .as_bytes()
            .get(offset)
            .copied()
            .and_then(hex_nibble)
            .ok_or_else(|| format!("{name} must use lowercase hexadecimal"))?;
        let low = value
            .as_bytes()
            .get(offset + 1)
            .copied()
            .and_then(hex_nibble)
            .ok_or_else(|| format!("{name} must use lowercase hexadecimal"))?;
        *byte = high << 4 | low;
    }
    Ok(decoded)
}

fn hex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn array_literal(bytes: &[u8]) -> String {
    let values = bytes
        .iter()
        .map(|byte| format!("0x{byte:02x}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{values}]")
}

fn write_runner_lock(bytes: &[u8]) -> Result<(), String> {
    let output = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| "Cargo did not provide OUT_DIR".to_owned())?;
    fs::write(output.join("runner.lock"), bytes)
        .map_err(|error| format!("could not write runner lock: {error}"))
}

fn write_generated(source: &str) -> Result<(), String> {
    let output = env::var_os("OUT_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| "Cargo did not provide OUT_DIR".to_owned())?;
    fs::write(output.join("compile_identity.rs"), source)
        .map_err(|error| format!("could not write generated identity: {error}"))
}
