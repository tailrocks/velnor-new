//! Exercise the stdin-only archive guard executable boundary.

#[path = "../build_support/archive_guard_inputs.rs"]
mod archive_guard_inputs;
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
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

#[test]
fn accepts_candidate_gzip_tar_from_stdin_without_environment() -> Result<(), Box<dyn Error>> {
    let input = gzip(&[0_u8; 1024])?;
    let output = invoke(&["candidate"], &input)?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    Ok(())
}

#[test]
fn accepts_cargo_package_gzip_tar_from_stdin_without_environment() -> Result<(), Box<dyn Error>> {
    let input = gzip(&[0_u8; 1024])?;
    let output = invoke(&["cargo-package"], &input)?;
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    Ok(())
}

#[test]
fn rejects_extra_arguments_and_retired_modes() -> Result<(), Box<dyn Error>> {
    let extra = invoke(&["candidate", "unexpected"], &[])?;
    assert!(!extra.status.success());
    assert!(String::from_utf8_lossy(&extra.stderr).contains("unexpected archive guard argument"));

    let retired = invoke(&["source"], &[])?;
    assert!(!retired.status.success());
    assert!(
        String::from_utf8_lossy(&retired.stderr).contains("unknown archive guard mode: source")
    );
    Ok(())
}

#[test]
fn reports_a_lowercase_source_fingerprint() -> Result<(), Box<dyn Error>> {
    let output = invoke(&["--fingerprint"], &[])?;
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    let fingerprint = String::from_utf8(output.stdout)?;
    assert_eq!(fingerprint.len(), 65);
    assert!(fingerprint.ends_with('\n'));
    assert!(
        fingerprint[..64]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("workspace root is missing")?;
    let (expected, _) = archive_guard_inputs::fingerprint(workspace)?;
    assert_eq!(fingerprint, format!("{expected}\n"));
    Ok(())
}

#[test]
fn source_tree_fingerprint_closure_includes_new_modules() -> Result<(), Box<dyn Error>> {
    let root = fixture_root()?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "mod new_module;\n")?;
    let before = archive_guard_inputs::collect_tree_files(&root, "src")?;
    fs::write(root.join("src/new_module.rs"), "pub fn value() {}\n")?;
    let after = archive_guard_inputs::collect_tree_files(&root, "src")?;
    assert!(!before.iter().any(|path| path == "src/new_module.rs"));
    assert!(after.iter().any(|path| path == "src/new_module.rs"));
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn source_tree_entry_discovery_is_bounded() -> Result<(), Box<dyn Error>> {
    let root = fixture_root()?;
    fs::create_dir_all(root.join("src"))?;
    for index in 0..513 {
        fs::write(root.join("src").join(format!("module-{index}.rs")), b"")?;
    }
    let result = archive_guard_inputs::collect_tree_files(&root, "src");
    fs::remove_dir_all(&root)?;
    let error = result.expect_err("oversized source tree was accepted");
    assert!(
        error
            .to_string()
            .contains("source tree entry count limit exceeded"),
        "unexpected error: {error}"
    );
    Ok(())
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

fn gzip(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(bytes)?;
    Ok(encoder.finish()?)
}
