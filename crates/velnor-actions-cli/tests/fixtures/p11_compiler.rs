//! P11 compiler rejection proofs: negative fixtures must fail the drivers.
//!
//! Each `p11_reject_*` fixture compiles clean at baseline (proving it is
//! otherwise valid) and fails once the workspace's denying lint level is
//! applied (proving the lint rejects exactly this shape). Every individual
//! deny in `[workspace.lints]` has one row below: adding a deny without a
//! fixture fails `extra_denies_match_tested_set`, removing one fails the
//! `root_denies` linkage. A positive control compiles clean under every
//! deny flag at once (proving valid code needs no allows). Enforcement
//! linkage: the workspace root must set each level the tests apply, read
//! through the structured TOML parser.

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

/// Document a fixture crate with extra rustdoc flags; cleanup never fails.
fn document(name: &str, flags: &[&str]) -> Result<Output, Box<dyn Error>> {
    let dir = fresh_dir("doc")?;
    let source = super::p11_toml::fixture(name)?;
    let file = dir.join("check.rs");
    std::fs::write(&file, source)?;
    let mut command = std::process::Command::new(tool("rustdoc")?);
    command
        .arg("--edition")
        .arg("2024")
        .arg("--crate-name")
        .arg("check")
        .arg("--out-dir")
        .arg(dir.join("out"))
        .args(flags)
        .arg(&file);
    let output = command.output()?;
    drop(std::fs::remove_dir_all(&dir));
    Ok(output)
}

/// Assert baseline success plus deny-flag failure naming the lint.
fn rejects_doc(name: &str, flag: &str, lint: &str) -> Result<(), Box<dyn Error>> {
    let base = document(name, &[])?;
    assert!(base.status.success(), "{name} doc baseline fails");
    let denied = document(name, &[flag])?;
    assert!(!denied.status.success(), "{name} doc accepted under {flag}");
    let stderr = String::from_utf8_lossy(&denied.stderr);
    assert!(
        stderr.contains(lint),
        "{name} doc failed without {lint}:\n{stderr}"
    );
    Ok(())
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
fn rejects(driver: &str, name: &str, flags: &[&str], lint: &str) -> Result<(), Box<dyn Error>> {
    let base = compile(driver, name, &[])?;
    assert!(base.status.success(), "{name} baseline fails");
    let denied = compile(driver, name, flags)?;
    assert!(!denied.status.success(), "{name} accepted under {flags:?}");
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
        &["--forbid=unsafe-code"],
        "unsafe-code",
    )
}

#[test]
fn reject_ignored_result_fails() -> Result<(), Box<dyn Error>> {
    root_denies("rust", "unused_must_use", "deny")?;
    rejects(
        "rustc",
        "p11_reject_must_use.rs",
        &["--deny=unused-must-use"],
        "unused-must-use",
    )
}

#[test]
fn reject_unwrap_fails() -> Result<(), Box<dyn Error>> {
    root_denies("clippy", "unwrap_used", "deny")?;
    rejects(
        "clippy-driver",
        "p11_reject_unwrap.rs",
        &["--deny=clippy::unwrap_used"],
        "unwrap_used",
    )
}

#[test]
fn reject_panic_fails() -> Result<(), Box<dyn Error>> {
    root_denies("clippy", "panic", "deny")?;
    rejects(
        "clippy-driver",
        "p11_reject_panic.rs",
        &["--deny=clippy::panic"],
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
        &["--deny=clippy::too_many_lines"],
        "too_many_lines",
    )
}

/// One extra clippy row: manifest key, fixture, deny flag, stderr marker.
const EXTRA_CLIPPY: [(&str, &str, &str, &str); 12] = [
    (
        "expect_used",
        "p11_reject_expect.rs",
        "--deny=clippy::expect_used",
        "expect_used",
    ),
    ("todo", "p11_reject_todo.rs", "--deny=clippy::todo", "todo"),
    (
        "unimplemented",
        "p11_reject_unimplemented.rs",
        "--deny=clippy::unimplemented",
        "unimplemented",
    ),
    (
        "dbg_macro",
        "p11_reject_dbg.rs",
        "--deny=clippy::dbg_macro",
        "dbg_macro",
    ),
    (
        "mem_forget",
        "p11_reject_mem_forget.rs",
        "--deny=clippy::mem_forget",
        "mem_forget",
    ),
    (
        "await_holding_lock",
        "p11_reject_await_lock.rs",
        "--deny=clippy::await_holding_lock",
        "await_holding_lock",
    ),
    (
        "await_holding_refcell_ref",
        "p11_reject_await_refcell.rs",
        "--deny=clippy::await_holding_refcell_ref",
        "await_holding_refcell_ref",
    ),
    (
        "let_underscore_future",
        "p11_reject_underscore_future.rs",
        "--deny=clippy::let_underscore_future",
        "let_underscore_future",
    ),
    (
        "let_underscore_must_use",
        "p11_reject_underscore_must_use.rs",
        "--deny=clippy::let_underscore_must_use",
        "let_underscore_must_use",
    ),
    (
        "undocumented_unsafe_blocks",
        "p11_reject_undoc_unsafe.rs",
        "--deny=clippy::undocumented_unsafe_blocks",
        "undocumented_unsafe_blocks",
    ),
    (
        "allow_attributes_without_reason",
        "p11_reject_allow_noreason.rs",
        "--deny=clippy::allow_attributes_without_reason",
        "allow_attributes_without_reason",
    ),
    (
        "allow_attributes",
        "p11_reject_allow_reasoned.rs",
        "--deny=clippy::allow_attributes",
        "allow_attributes",
    ),
];

#[test]
fn reject_each_extra_clippy_deny_fails() -> Result<(), Box<dyn Error>> {
    for (key, name, flag, marker) in EXTRA_CLIPPY {
        let level = if key == "allow_attributes" {
            "warn"
        } else {
            "deny"
        };
        root_denies("clippy", key, level)?;
        rejects("clippy-driver", name, &[flag], marker)?;
    }
    Ok(())
}

#[test]
fn reject_each_extra_rust_deny_fails() -> Result<(), Box<dyn Error>> {
    root_denies("rust", "unexpected_cfgs", "deny")?;
    // Bare rustc enables no `--check-cfg`, so the unknown name is only
    // unexpected once the flag below declares the known set; the marker is
    // the diagnostic text because rustc never prints the lint name here.
    rejects(
        "rustc",
        "p11_reject_unexpected_cfg.rs",
        &[
            "--check-cfg=cfg(feature, values(\"p11none\"))",
            "--deny=unexpected_cfgs",
        ],
        "unexpected `cfg` condition",
    )?;
    root_denies("rust", "unfulfilled_lint_expectations", "deny")?;
    rejects(
        "rustc",
        "p11_reject_unfulfilled_expect.rs",
        &["--deny=unfulfilled-lint-expectations"],
        "unfulfilled",
    )
}

#[test]
fn reject_each_rustdoc_deny_fails() -> Result<(), Box<dyn Error>> {
    root_denies("rustdoc", "broken_intra_doc_links", "deny")?;
    rejects_doc(
        "p11_reject_broken_link.rs",
        "--deny=rustdoc::broken-intra-doc-links",
        "rustdoc::broken-intra-doc-links",
    )?;
    root_denies("rustdoc", "private_intra_doc_links", "deny")?;
    rejects_doc(
        "p11_reject_private_link.rs",
        "--deny=rustdoc::private-intra-doc-links",
        "rustdoc::private-intra-doc-links",
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
        (
            "rustc",
            &[
                "--forbid=unsafe-code",
                "--deny=unused-must-use",
                "--deny=unexpected_cfgs",
                "--deny=unfulfilled-lint-expectations",
            ] as &[&str],
        ),
        (
            "clippy-driver",
            &[
                "--deny=clippy::unwrap_used",
                "--deny=clippy::expect_used",
                "--deny=clippy::panic",
                "--deny=clippy::todo",
                "--deny=clippy::unimplemented",
                "--deny=clippy::dbg_macro",
                "--deny=clippy::mem_forget",
                "--deny=clippy::await_holding_lock",
                "--deny=clippy::await_holding_refcell_ref",
                "--deny=clippy::let_underscore_future",
                "--deny=clippy::let_underscore_must_use",
                "--deny=clippy::undocumented_unsafe_blocks",
                "--deny=clippy::allow_attributes_without_reason",
                "--deny=clippy::allow_attributes",
                "--deny=clippy::too_many_lines",
            ] as &[&str],
        ),
    ] {
        let output = compile(driver, "p11_accept_clean.rs", flags)?;
        assert!(
            output.status.success(),
            "{driver} rejects valid code:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}
