// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, ExitCode};

use sha2::{Digest, Sha256};

const HEX: &[u8; 16] = b"0123456789abcdef";

pub(super) fn create_absent() -> io::Result<()> {
    let root = super::executable::private_mutation_root()?;
    let requested = env::var_os("VELNOR_GOLDEN_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("golden directory is missing"))?;
    let parent = requested
        .parent()
        .ok_or_else(|| io::Error::other("missing parent"))?
        .canonicalize()?;
    if parent != root {
        return Err(io::Error::other(
            "golden directory is not a private fixture",
        ));
    }
    match fs::create_dir(&requested) {
        Ok(()) => fs::write(requested.join("sentinel"), b"candidate-created\n"),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error),
    }
}

pub(super) fn mutate_extra(create: bool) -> io::Result<()> {
    let root = super::executable::private_mutation_root()?;
    let requested = env::var_os("VELNOR_GOLDEN_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("golden directory is missing"))?;
    let oracle = fs::canonicalize(&requested)?;
    if oracle != requested || !oracle.starts_with(&root) || oracle == root {
        return Err(io::Error::other(
            "golden directory is not a private fixture",
        ));
    }
    fs::write(
        oracle.join(if create { "new-file" } else { "README.md" }),
        b"candidate mutation\n",
    )
}

pub(super) fn forge_after_generate() -> ExitCode {
    match run_and_forge() {
        Ok(output) => output,
        Err(error) => {
            eprintln!("qualification diagnostics oracle forge failed: {error}");
            ExitCode::from(56)
        }
    }
}

fn run_and_forge() -> io::Result<ExitCode> {
    let current = env::current_exe()?;
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    let output = Command::new(current)
        .args(&args)
        .env_remove("VELNOR_PLAN_DIAGNOSTICS_MODE")
        .output()?;
    io::stdout().write_all(&output.stdout)?;
    io::stderr().write_all(&output.stderr)?;
    if !output.status.success() {
        return Ok(output
            .status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .map_or(ExitCode::FAILURE, ExitCode::from));
    }
    let current_dir = fs::canonicalize(env::current_dir()?)?;
    if current_dir.file_name().is_some_and(|name| name == "nested") {
        let destination = output_directory(&args)?;
        let generated = destination.join(".github/CLAUDE.md");
        let mut bytes = fs::read(&generated)?;
        bytes.extend_from_slice(b"qualification-diagnostics-oracle-forge\n");
        fs::write(&generated, &bytes)?;
        update_oracle(&bytes)?;
    }
    Ok(ExitCode::SUCCESS)
}

fn output_directory(args: &[std::ffi::OsString]) -> io::Result<PathBuf> {
    args.windows(2)
        .find(|pair| pair[0] == "--output-dir")
        .map(|pair| PathBuf::from(&pair[1]))
        .ok_or_else(|| io::Error::other("generate output directory is missing"))
}

fn update_oracle(bytes: &[u8]) -> io::Result<()> {
    let root = super::executable::private_mutation_root()?;
    let requested = env::var_os("VELNOR_GOLDEN_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("golden directory is missing"))?;
    let oracle = fs::canonicalize(&requested)?;
    if oracle != requested || !oracle.starts_with(&root) || oracle == root {
        return Err(io::Error::other(
            "golden directory is not a private fixture",
        ));
    }
    let target = oracle.join("cases/nested/preview/.github/CLAUDE.md");
    if !fs::symlink_metadata(&target)?.file_type().is_file() {
        return Err(io::Error::other("golden target is not a regular file"));
    }
    fs::write(&target, bytes)?;
    let tree = oracle.join("cases/nested/tree.sha256");
    let tree_bytes = update_tree_digest(&tree, bytes)?;
    update_manifest(&oracle, "./nested/preview/.github/CLAUDE.md", bytes)?;
    update_manifest(&oracle, "./nested/tree.sha256", &tree_bytes)
}

fn update_tree_digest(path: &std::path::Path, bytes: &[u8]) -> io::Result<Vec<u8>> {
    let manifest = fs::read_to_string(path)?;
    let updated = update_entry(&manifest, "./.github/CLAUDE.md", bytes)?;
    fs::write(path, &updated)?;
    Ok(updated.into_bytes())
}

fn update_manifest(oracle: &std::path::Path, target: &str, bytes: &[u8]) -> io::Result<()> {
    let path = oracle.join("MANIFEST.sha256");
    let manifest = fs::read_to_string(&path)?;
    let updated = update_entry(&manifest, target, bytes)?;
    fs::write(path, updated)
}

fn update_entry(manifest: &str, target: &str, bytes: &[u8]) -> io::Result<String> {
    let mut digest = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        digest.push(char::from(HEX[usize::from(byte >> 4)]));
        digest.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    let mut found = false;
    let mut updated = String::new();
    for line in manifest.lines() {
        if let Some((metadata, name)) = line.split_once("  ")
            && name == target
        {
            let mode = metadata
                .split_whitespace()
                .next()
                .ok_or_else(|| io::Error::other("golden manifest mode is missing"))?;
            updated.push_str(mode);
            updated.push(' ');
            updated.push_str(&digest);
            updated.push_str("  ");
            updated.push_str(target);
            updated.push('\n');
            found = true;
        } else {
            updated.push_str(line);
            updated.push('\n');
        }
    }
    if !found {
        return Err(io::Error::other("golden target is missing from manifest"));
    }
    Ok(updated)
}
