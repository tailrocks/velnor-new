//! Exercise the stdin-only archive guard executable boundary.

#[path = "../build_support/archive_guard_inputs.rs"]
mod archive_guard_inputs;
use std::error::Error;
use std::ffi::OsStr;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
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

fn invoke_extract(arguments: &[&OsStr]) -> Result<std::process::Output, Box<dyn Error>> {
    Ok(Command::new(GUARD)
        .args(arguments)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?)
}

fn invoke_source_fingerprint(root: &Path) -> Result<std::process::Output, Box<dyn Error>> {
    Ok(Command::new(GUARD)
        .arg("source-fingerprint")
        .arg(root)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()?)
}

#[test]
fn extracts_only_expected_candidate_members_with_exact_modes() -> Result<(), Box<dyn Error>> {
    let root = fixture_root()?;
    let destination = root.join("candidate");
    fs::create_dir(&destination)?;
    fs::write(root.join("jackin"), b"binary")?;
    fs::set_permissions(root.join("jackin"), fs::Permissions::from_mode(0o755))?;
    fs::write(root.join("SHA256SUMS"), b"digest")?;
    fs::set_permissions(root.join("SHA256SUMS"), fs::Permissions::from_mode(0o644))?;
    fs::write(root.join("provenance.json"), b"statement")?;
    fs::set_permissions(
        root.join("provenance.json"),
        fs::Permissions::from_mode(0o644),
    )?;
    let archive = root.join("candidate.tar");
    let output = Command::new("tar")
        .args([
            "--format=ustar",
            "-cf",
            "candidate.tar",
            "jackin",
            "SHA256SUMS",
            "provenance.json",
        ])
        .current_dir(&root)
        .output()?;
    assert!(output.status.success());

    let archive_link = root.join("candidate-link.tar");
    std::os::unix::fs::symlink(&archive, &archive_link)?;
    let symlink_result = invoke_extract(&[
        OsStr::new("extract-candidate"),
        archive_link.as_os_str(),
        destination.as_os_str(),
        OsStr::new("jackin"),
        OsStr::new("SHA256SUMS"),
        OsStr::new("provenance.json"),
    ])?;
    assert!(!symlink_result.status.success());
    assert_eq!(fs::read_dir(&destination)?.count(), 0);

    let result = invoke_extract(&[
        OsStr::new("extract-candidate"),
        archive.as_os_str(),
        destination.as_os_str(),
        OsStr::new("jackin"),
        OsStr::new("SHA256SUMS"),
        OsStr::new("provenance.json"),
    ])?;
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read(destination.join("jackin"))?, b"binary");
    assert_eq!(fs::read(destination.join("SHA256SUMS"))?, b"digest");
    assert_eq!(fs::read(destination.join("provenance.json"))?, b"statement");
    assert_eq!(
        fs::metadata(destination.join("jackin"))?
            .permissions()
            .mode()
            & 0o777,
        0o755
    );
    assert_eq!(
        fs::metadata(destination.join("SHA256SUMS"))?
            .permissions()
            .mode()
            & 0o777,
        0o644
    );
    assert_eq!(fs::read_dir(destination)?.count(), 3);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn extraction_rejects_existing_destination_without_replacing_it() -> Result<(), Box<dyn Error>> {
    let root = fixture_root()?;
    let destination = root.join("candidate");
    fs::create_dir(&destination)?;
    fs::write(destination.join("jackin"), b"keep")?;
    fs::write(root.join("jackin"), b"binary")?;
    fs::set_permissions(root.join("jackin"), fs::Permissions::from_mode(0o755))?;
    fs::write(root.join("SHA256SUMS"), b"digest")?;
    fs::write(root.join("provenance.json"), b"statement")?;
    fs::set_permissions(root.join("SHA256SUMS"), fs::Permissions::from_mode(0o644))?;
    fs::set_permissions(
        root.join("provenance.json"),
        fs::Permissions::from_mode(0o644),
    )?;
    let output = Command::new("tar")
        .args([
            "--format=ustar",
            "-cf",
            "candidate.tar",
            "jackin",
            "SHA256SUMS",
            "provenance.json",
        ])
        .current_dir(&root)
        .output()?;
    assert!(output.status.success());
    let archive = root.join("candidate.tar");
    let result = invoke_extract(&[
        OsStr::new("extract-candidate"),
        archive.as_os_str(),
        destination.as_os_str(),
        OsStr::new("jackin"),
        OsStr::new("SHA256SUMS"),
        OsStr::new("provenance.json"),
    ])?;
    assert!(!result.status.success());
    assert_eq!(fs::read(destination.join("jackin"))?, b"keep");
    assert_eq!(fs::read_dir(destination)?.count(), 1);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn accepts_release_producer_raw_tar_from_stdin_without_environment() -> Result<(), Box<dyn Error>> {
    let input = producer_tar()?;
    let output = invoke(&["candidate"], &input)?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, Vec::<u8>::new());
    Ok(())
}

#[test]
fn rejects_gzip_candidate_and_raw_cargo_package_without_format_sniffing()
-> Result<(), Box<dyn Error>> {
    let raw = producer_tar()?;
    let compressed = gzip(&raw)?;

    let candidate = invoke(&["candidate"], &compressed)?;
    assert!(!candidate.status.success());
    assert!(!String::from_utf8_lossy(&candidate.stderr).is_empty());

    let cargo_package = invoke(&["cargo-package"], &raw)?;
    assert!(!cargo_package.status.success());
    assert!(!String::from_utf8_lossy(&cargo_package.stderr).is_empty());
    Ok(())
}

#[test]
fn accepts_cargo_package_gzip_tar_from_stdin_without_environment() -> Result<(), Box<dyn Error>> {
    let input = gzip(&producer_tar()?)?;
    let output = invoke(&["cargo-package"], &input)?;
    assert!(output.status.success());
    assert_eq!(output.stdout, Vec::<u8>::new());
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
    assert_eq!(output.stderr, Vec::<u8>::new());
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

fn fingerprint_tree_files(root: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    fs::create_dir_all(root.join("scripts"))?;
    fs::write(root.join("scripts/archive-guard-inputs.txt"), "tree src\n")?;
    let (_, watched) = archive_guard_inputs::fingerprint(root)?;
    watched
        .into_iter()
        .map(|path| Ok(path.strip_prefix(root)?.to_string_lossy().into_owned()))
        .collect()
}

#[test]
fn source_tree_fingerprint_closure_includes_new_modules() -> Result<(), Box<dyn Error>> {
    let root = fixture_root()?;
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/lib.rs"), "mod new_module;\n")?;
    let before = fingerprint_tree_files(&root)?;
    fs::write(root.join("src/new_module.rs"), "pub fn value() {}\n")?;
    let after = fingerprint_tree_files(&root)?;
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
    let result = fingerprint_tree_files(&root);
    fs::remove_dir_all(&root)?;
    let error = result.expect_err("oversized source tree was accepted");
    assert!(
        error
            .to_string()
            .contains("source file count limit exceeded"),
        "unexpected error: {error}"
    );
    Ok(())
}

#[test]
fn native_source_fingerprint_command_matches_build_algorithm() -> Result<(), Box<dyn Error>> {
    let root = fixture_root()?;
    fs::create_dir(root.join("scripts"))?;
    fs::write(
        root.join("scripts/archive-guard-inputs.txt"),
        b"file src.rs\n",
    )?;
    fs::write(root.join("src.rs"), b"pub fn fixture() {}\n")?;
    let (expected, _) = archive_guard_inputs::fingerprint(&root)?;
    let output = invoke_source_fingerprint(&root)?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stderr, Vec::<u8>::new());
    assert_eq!(output.stdout, format!("{expected}\n").as_bytes());
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn native_source_fingerprint_rejects_symlink_and_fifo_inputs() -> Result<(), Box<dyn Error>> {
    let root = fixture_root()?;
    fs::create_dir(root.join("scripts"))?;
    fs::write(
        root.join("scripts/archive-guard-inputs.txt"),
        b"file input\n",
    )?;
    let outside = root.join("outside");
    fs::write(&outside, b"held bytes")?;
    std::os::unix::fs::symlink(&outside, root.join("input"))?;
    let symlink = invoke_source_fingerprint(&root)?;
    assert!(!symlink.status.success());
    assert_eq!(symlink.stdout, Vec::<u8>::new());
    fs::remove_file(root.join("input"))?;
    let fifo = root.join("input");
    let output = Command::new("mkfifo").arg(&fifo).output()?;
    assert!(output.status.success());
    let fifo_result = invoke_source_fingerprint(&root)?;
    assert!(!fifo_result.status.success());
    assert_eq!(fifo_result.stdout, Vec::<u8>::new());
    fs::remove_dir_all(root)?;
    Ok(())
}

fn fixture_root() -> Result<PathBuf, Box<dyn Error>> {
    let serial = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "velnor-archive-guard-inputs-{}-{serial}",
        std::process::id()
    ));
    fs::create_dir(&root)?;
    Ok(fs::canonicalize(root)?)
}

fn producer_tar() -> Result<Vec<u8>, Box<dyn Error>> {
    let root = fixture_root()?;
    fs::write(root.join("binary"), b"release binary")?;
    fs::write(root.join("checksum"), b"sha256:fixture")?;
    fs::write(root.join("provenance"), b"source=commit")?;
    let archive = root.join("generator-assets.tar");
    let output = Command::new("tar")
        .args([
            "--format=ustar",
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

#[path = "archive_guard_input_bounds.rs"]
mod input_bounds;

#[path = "archive_guard_qualification_cli.rs"]
mod qualification_cli;
