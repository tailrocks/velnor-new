//! Exercise the stdin-only archive guard executable boundary.

#[path = "../build_support/archive_guard_inputs.rs"]
mod archive_guard_inputs;
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use flate2::Compression;
use flate2::write::GzEncoder;

const GUARD: &str = env!("CARGO_BIN_EXE_velnor-archive-guard");
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

fn invoke(arguments: &[&str], input: &[u8]) -> Result<std::process::Output, Box<dyn Error>> {
    let mut child = Command::new(GUARD)
        .args(arguments)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take()
        && let Err(error) = stdin.write_all(input)
        && error.kind() != std::io::ErrorKind::BrokenPipe
    {
        return Err(error.into());
    }
    Ok(child.wait_with_output()?)
}

fn fixture_root() -> Result<PathBuf, Box<dyn Error>> {
    let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "velnor-archive-guard-inputs-{}-{serial}",
        std::process::id()
    ));
    fs::create_dir(&root)?;
    Ok(root)
}

fn producer_tar() -> Result<Vec<u8>, Box<dyn Error>> {
    let root = fixture_root()?;
    fs::write(root.join("binary"), b"release binary")?;
    fs::write(root.join("checksum"), b"sha256:fixture")?;
    fs::write(root.join("provenance"), b"source=commit")?;
    let archive = root.join("generator-assets.tar");
    let output = Command::new("tar")
        .args([
            "-cf",
            "generator-assets.tar",
            "binary",
            "checksum",
            "provenance",
        ])
        .current_dir(&root)
        .output()?;
    if !output.status.success() {
        fs::remove_dir_all(&root)?;
        return Err(std::io::Error::other(format!(
            "release tar producer failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let bytes = fs::read(archive)?;
    fs::remove_dir_all(root)?;
    Ok(bytes)
}

fn gzip(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes)?;
    Ok(encoder.finish()?)
}
mod boundary_tests;
mod tree_tests;
