use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use velnor_actions_mise::inventory_loader::compiled_inventory_loader;

type TestResult = Result<(), Box<dyn Error>>;
static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Result<Self, Box<dyn Error>> {
        let path = std::env::temp_dir().join(format!(
            "velnor-inventory-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path)?;
        Ok(Self(path.canonicalize()?))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("inventory fixture cleanup failed: {error}");
        }
    }
}

enum Observation {
    SyntheticInventory,
    RawMetadata,
    StrictInventory,
    UnqualifiedArchive,
}

fn inventory(
    owner: &Path,
    roots: &str,
    observation: Observation,
) -> Result<Output, Box<dyn Error>> {
    let entry = match observation {
        Observation::SyntheticInventory => SYNTHETIC_ENTRY,
        Observation::RawMetadata => METADATA_ENTRY,
        Observation::StrictInventory => STRICT_ENTRY,
        Observation::UnqualifiedArchive => UNQUALIFIED_ENTRY,
    };
    let source = format!("{}\n{entry}", compiled_inventory_loader()?);
    let mut child = Command::new("/usr/bin/python3")
        .args(["-I", "-S", "-"])
        .arg(owner)
        .arg(roots)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .ok_or("missing inventory stdin")?
        .write_all(source.as_bytes())?;
    Ok(child.wait_with_output()?)
}

// Synthetic metadata bypass isolates strict schema2 walker semantics only.
// These fixtures neither admit real whole manifests nor grant archive authority.
const SYNTHETIC_ENTRY: &str = r#"
import source_archive_inventory_leaf
from source_archive_inventory import _inventory
source_archive_inventory_leaf.metadata_records = lambda *_args: []
_result = _inventory(sys.argv[1], json.loads(sys.argv[2]))
print(_result.digest, _result.files, _result.bytes)
"#;

const METADATA_ENTRY: &str = r#"
import os
from opaque_inventory_metadata import metadata_records
_root = json.loads(sys.argv[2])[0]
_path = os.path.join(sys.argv[1], _root)
_records = metadata_records(_path, os.stat(_path, follow_symlinks=False), 'record')
print(json.dumps([record.hex() for record in _records], separators=(',', ':')))
"#;

const STRICT_ENTRY: &str = r#"
from source_archive_inventory import _inventory
_result = _inventory(sys.argv[1], json.loads(sys.argv[2]))
print(_result.digest, _result.files, _result.bytes)
"#;

const UNQUALIFIED_ENTRY: &str = r#"
from source_archive_inventory import source_archive_inventory
source_archive_inventory(None, None)
"#;

fn summary(output: Output) -> Result<String, Box<dyn Error>> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(String::from_utf8(output.stdout)?)
}

#[test]
fn synthetic_metadata_bypassed_inventory_observes_bytes_and_newline_names() -> TestResult {
    let fixture = Fixture::new()?;
    fs::create_dir(fixture.0.join("payload"))?;
    let file = fixture.0.join("payload/not-a-script\nname");
    fs::write(&file, b"\0\xff\nexit 97\n")?;
    let first = summary(inventory(
        &fixture.0,
        "[\"payload\"]",
        Observation::SyntheticInventory,
    )?)?;
    assert!(first.ends_with(" 1 11\n"));
    assert_eq!(
        first,
        summary(inventory(
            &fixture.0,
            "[\"payload\"]",
            Observation::SyntheticInventory
        )?)?
    );
    fs::write(&file, b"\0\xff\nexit 98\n")?;
    assert_ne!(
        first,
        summary(inventory(
            &fixture.0,
            "[\"payload\"]",
            Observation::SyntheticInventory
        )?)?
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn synthetic_metadata_bypassed_inventory_observes_modes_and_confines_links() -> TestResult {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let fixture = Fixture::new()?;
    fs::create_dir(fixture.0.join("payload"))?;
    let file = fixture.0.join("payload/file");
    fs::write(&file, b"opaque")?;
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600))?;
    let first = summary(inventory(
        &fixture.0,
        "[\"payload\"]",
        Observation::SyntheticInventory,
    )?)?;
    fs::set_permissions(&file, fs::Permissions::from_mode(0o700))?;
    assert_ne!(
        first,
        summary(inventory(
            &fixture.0,
            "[\"payload\"]",
            Observation::SyntheticInventory
        )?)?
    );
    let link = fixture.0.join("payload/link");
    symlink("file", &link)?;
    assert!(
        inventory(&fixture.0, "[\"payload\"]", Observation::SyntheticInventory)?
            .status
            .success()
    );
    assert!(
        !inventory(&fixture.0, "[\"payload\"]", Observation::UnqualifiedArchive)?
            .status
            .success()
    );
    fs::remove_file(&link)?;
    fs::write(fixture.0.join("unowned"), b"outside selected root")?;
    symlink("../unowned", &link)?;
    assert!(
        !inventory(&fixture.0, "[\"payload\"]", Observation::SyntheticInventory)?
            .status
            .success()
    );
    Ok(())
}

#[test]
fn synthetic_metadata_bypassed_inventory_rejects_invalid_roots() -> TestResult {
    let fixture = Fixture::new()?;
    for roots in ["[\"../escape\"]", "[\"/tmp\"]", "[\"payload\",\"payload\"]"] {
        assert!(
            !inventory(&fixture.0, roots, Observation::SyntheticInventory)?
                .status
                .success()
        );
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn actual_metadata_xattr_observation_and_strict_archive_rejection() -> TestResult {
    let fixture = Fixture::new()?;
    let file = fixture.0.join("payload");
    fs::write(&file, b"opaque")?;
    let first = summary(inventory(
        &fixture.0,
        "[\"payload\"]",
        Observation::RawMetadata,
    )?)?;
    #[cfg(target_os = "linux")]
    let set = Command::new("/usr/bin/python3")
        .args([
            "-I",
            "-S",
            "-c",
            "import os,sys; os.setxattr(sys.argv[1], 'user.velnor-test', b'opaque metadata')",
        ])
        .arg(&file)
        .output()?;
    #[cfg(target_os = "macos")]
    let set = Command::new("/usr/bin/xattr")
        .args(["-w", "com.velnor.test", "opaque metadata"])
        .arg(&file)
        .output()?;
    assert!(
        set.status.success(),
        "{}",
        String::from_utf8_lossy(&set.stderr)
    );
    assert_ne!(
        first,
        summary(inventory(
            &fixture.0,
            "[\"payload\"]",
            Observation::RawMetadata
        )?)?
    );
    let strict = inventory(&fixture.0, "[\"payload\"]", Observation::StrictInventory)?;
    assert!(!strict.status.success());
    assert!(String::from_utf8_lossy(&strict.stderr).contains("payload_unsupported_metadata"));
    let rejected = inventory(&fixture.0, "[\"payload\"]", Observation::UnqualifiedArchive)?;
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("source_archive_projection_unqualified")
    );
    Ok(())
}
