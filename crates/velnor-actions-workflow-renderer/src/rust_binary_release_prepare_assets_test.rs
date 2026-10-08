use std::error::Error;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use super::scripts;

const PACKAGE: &str = "demo-package";
const BINARY: &str = "demo-binary";

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Result<Self, Box<dyn Error>> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let root = std::env::temp_dir().join(format!(
            "velnor-binary-release-assets-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root)?;
        Ok(Self(root))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

#[test]
fn prepare_assets_accepts_expected_archives_and_rejects_invalid_shapes()
-> Result<(), Box<dyn Error>> {
    let script = scripts::prepare_assets(PACKAGE, BINARY);
    let accepted_root = Scratch::new()?;
    write_inputs(&accepted_root.0, Archive::Expected, Archive::Expected)?;
    let accepted = run_prepare(&script, &accepted_root.0)?;
    assert!(accepted.status.success(), "{}", stderr(&accepted));
    let sums = fs::read_to_string(accepted_root.0.join("assets/SHA256SUMS"))?;
    assert!(sums.contains("demo-binary-1.10.0-x86_64-unknown-linux-gnu.tar.gz"));
    assert!(sums.contains("demo-binary-1.10.0-aarch64-apple-darwin.tar.gz"));

    for invalid in [
        Archive::ExtraEntry,
        Archive::WrongPath,
        Archive::WrongMode,
        Archive::Malformed,
    ] {
        let root = Scratch::new()?;
        write_inputs(&root.0, invalid, Archive::Expected)?;
        let rejected = run_prepare(&script, &root.0)?;
        assert!(!rejected.status.success(), "{invalid:?} was accepted");
    }
    Ok(())
}

#[derive(Clone, Copy, Debug)]
enum Archive {
    Expected,
    ExtraEntry,
    WrongPath,
    WrongMode,
    Malformed,
}

fn write_inputs(root: &Path, linux: Archive, macos: Archive) -> Result<(), Box<dyn Error>> {
    write_archive(root, "incoming-linux", "x86_64-unknown-linux-gnu", linux)?;
    write_archive(root, "incoming-macos", "aarch64-apple-darwin", macos)
}

fn write_archive(
    root: &Path,
    directory: &str,
    target: &str,
    shape: Archive,
) -> Result<(), Box<dyn Error>> {
    let incoming = root.join("assets").join(directory);
    let archive = incoming.join(format!("{BINARY}-1.10.0-{target}.tar.gz"));
    fs::create_dir_all(&incoming)?;
    if matches!(shape, Archive::Malformed) {
        fs::write(archive, b"not a gzip tar archive")?;
        return Ok(());
    }
    let source = root.join(format!("source-{directory}"));
    fs::create_dir_all(&source)?;
    let binary = source.join(BINARY);
    fs::write(&binary, b"binary fixture")?;
    let mode = if matches!(shape, Archive::WrongMode) {
        0o644
    } else {
        0o755
    };
    fs::set_permissions(&binary, fs::Permissions::from_mode(mode))?;
    match shape {
        Archive::Expected | Archive::WrongMode => tar(&source, &archive, &[BINARY]),
        Archive::ExtraEntry => {
            fs::write(source.join("extra"), b"extra")?;
            tar(&source, &archive, &[BINARY, "extra"])
        }
        Archive::WrongPath => {
            let nested = source.join("nested");
            fs::create_dir_all(&nested)?;
            fs::rename(&binary, nested.join(BINARY))?;
            tar(&source, &archive, &["nested/demo-binary"])
        }
        Archive::Malformed => unreachable!(),
    }
}

fn tar(source: &Path, archive: &Path, entries: &[&str]) -> Result<(), Box<dyn Error>> {
    let status = Command::new("tar")
        .arg("-czf")
        .arg(archive)
        .arg("-C")
        .arg(source)
        .args(entries)
        .status()?;
    if !status.success() {
        return Err("failed to create archive fixture".into());
    }
    Ok(())
}

fn run_prepare(script: &str, root: &Path) -> Result<Output, Box<dyn Error>> {
    Ok(Command::new("bash")
        .args(["-c", script])
        .current_dir(root)
        .env("SOURCE_SHA", "0123456789012345678901234567890123456789")
        .env("RELEASE_TAG", format!("{PACKAGE}-v1.10.0"))
        .env("RELEASE_VERSION", "1.10.0")
        .env_remove("GH_TOKEN")
        .output()?)
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}
