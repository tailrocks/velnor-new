//! P11 compiler rejection proofs: negative fixtures must fail the drivers.
//!
//! Each `p11_reject_*` fixture compiles clean at baseline (proving it is
//! otherwise valid) and fails once the workspace's denying lint level is
//! applied (proving the lint rejects exactly this shape). A positive
//! control compiles clean under every deny flag at once (proving valid
//! code needs no allows). Enforcement linkage: the workspace root must set
//! each level the tests apply, read through the structured TOML parser.

use std::error::Error;
use std::path::PathBuf;
use std::process::Output;
use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonic counter keeping scratch dirs unique within one test binary.
static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Resolve a driver from `PATH`; missing drivers fail closed, never skip.
fn tool(name: &str) -> Result<PathBuf, Box<dyn Error>> {
    let paths = std::env::var("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(format!("{name} not on PATH").into())
}

/// Fresh unique scratch directory under the system temp dir.
fn fresh_dir(prefix: &str) -> Result<PathBuf, Box<dyn Error>> {
    let id = COUNTER.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("p11-{prefix}-{}-{id}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Compile a fixture lib with extra driver flags; cleanup never fails.
fn compile(driver: &str, name: &str, flags: &[&str]) -> Result<Output, Box<dyn Error>> {
    let dir = fresh_dir("compile")?;
    let source = super::p11_toml::fixture(name)?;
    let file = dir.join("check.rs");
    std::fs::write(&file, source)?;
    let mut command = std::process::Command::new(tool(driver)?);
    command
        .arg("--edition")
        .arg("2024")
        .arg("--crate-type")
        .arg("lib")
        .arg("--out-dir")
        .arg(&dir)
        .args(flags)
        .arg(&file);
    let output = command.output()?;
    drop(std::fs::remove_dir_all(&dir));
    Ok(output)
}

/// Assert the workspace root denies `lint` in the `rust` or `clippy` table.
fn root_denies(table: &str, lint: &str, level: &str) -> Result<(), Box<dyn Error>> {
    let root = super::p11_toml::parse(&super::read("Cargo.toml")?)?;
    let section = format!("workspace.lints.{table}");
    let table = super::p11_toml::section(&root, &section).ok_or_else(|| section.clone())?;
    let actual = super::p11_toml::value(table, lint);
    assert_eq!(actual.as_deref(), Some(level), "{lint} level drift");
    Ok(())
}

/// Assert baseline success plus deny-flag failure naming the lint.
fn rejects(driver: &str, name: &str, flag: &str, lint: &str) -> Result<(), Box<dyn Error>> {
    let base = compile(driver, name, &[])?;
    assert!(base.status.success(), "{name} baseline fails");
    let denied = compile(driver, name, &[flag])?;
    assert!(!denied.status.success(), "{name} accepted under {flag}");
    let stderr = String::from_utf8_lossy(&denied.stderr);
    assert!(
        stderr.contains(lint),
        "{name} failed without {lint}:\n{stderr}"
    );
    Ok(())
}

#[test]
fn reject_unsafe_fails() -> Result<(), Box<dyn Error>> {
    root_denies("rust", "unsafe_code", "forbid")?;
    let body = super::p11_toml::fixture("p11_reject_unsafe.rs")?;
    assert!(body.contains("unsafe"), "fixture lost its unsafe block");
    rejects(
        "rustc",
        "p11_reject_unsafe.rs",
        "--forbid=unsafe-code",
        "unsafe-code",
    )
}

#[test]
fn reject_ignored_result_fails() -> Result<(), Box<dyn Error>> {
    root_denies("rust", "unused_must_use", "deny")?;
    rejects(
        "rustc",
        "p11_reject_must_use.rs",
        "--deny=unused-must-use",
        "unused-must-use",
    )
}

#[test]
fn reject_unwrap_fails() -> Result<(), Box<dyn Error>> {
    root_denies("clippy", "unwrap_used", "deny")?;
    rejects(
        "clippy-driver",
        "p11_reject_unwrap.rs",
        "--deny=clippy::unwrap_used",
        "unwrap_used",
    )
}

#[test]
fn reject_panic_fails() -> Result<(), Box<dyn Error>> {
    root_denies("clippy", "panic", "deny")?;
    rejects(
        "clippy-driver",
        "p11_reject_panic.rs",
        "--deny=clippy::panic",
        "clippy::panic",
    )
}

#[test]
fn reject_long_fn_fails() -> Result<(), Box<dyn Error>> {
    root_denies("clippy", "too_many_lines", "deny")?;
    let clippy = super::p11_toml::parse(&super::read("clippy.toml")?)?;
    let bare = super::p11_toml::section(&clippy, "").ok_or("no bare keys")?;
    let threshold = super::p11_toml::value(bare, "too-many-lines-threshold");
    assert_eq!(threshold.as_deref(), Some("80"), "threshold drift");
    let body = super::p11_toml::fixture("p11_reject_long_fn.rs")?;
    assert!(body.lines().count() > 100, "fixture below default gate");
    rejects(
        "clippy-driver",
        "p11_reject_long_fn.rs",
        "--deny=clippy::too_many_lines",
        "too_many_lines",
    )
}

#[test]
fn reject_oversize_file_fails() -> Result<(), Box<dyn Error>> {
    let body = super::p11_toml::fixture("p11_reject_oversize.rs.txt")?;
    let lines = body.bytes().filter(|byte| *byte == b'\n').count();
    assert!(lines > 400, "fixture is only {lines} lines");
    let row = super::alint_miniyaml::EXPECTED
        .iter()
        .find(|row| row.id == "rust-max-lines")
        .ok_or("no rust-max-lines pin")?;
    assert!(row.pairs.contains(&("max_lines", "400")), "limit drift");
    Ok(())
}

#[test]
fn accept_clean_passes_every_deny() -> Result<(), Box<dyn Error>> {
    let body = super::p11_toml::fixture("p11_accept_clean.rs")?;
    for marker in ["#[allow", "allow("] {
        assert!(!body.contains(marker), "control must not use allows");
    }
    for (driver, flags) in [
        ("rustc", ["--forbid=unsafe-code", "--deny=unused-must-use"]),
        (
            "clippy-driver",
            ["--deny=clippy::unwrap_used", "--deny=clippy::panic"],
        ),
    ] {
        let output = compile(driver, "p11_accept_clean.rs", &flags)?;
        assert!(
            output.status.success(),
            "{driver} rejects valid code:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}
