//! Aggregate source-input bounds shared by native and stage-zero admission.

use std::{error::Error, fs, path::Path, process::Command};

use super::{archive_guard_inputs, fixture_root};

fn tree_fixture(files: usize, manifest: &str) -> Result<std::path::PathBuf, Box<dyn Error>> {
    let root = fixture_root()?;
    fs::create_dir(root.join("scripts"))?;
    fs::create_dir(root.join("src"))?;
    fs::write(root.join("scripts/archive-guard-inputs.txt"), manifest)?;
    for index in 0..files {
        fs::write(root.join("src").join(format!("module-{index}.rs")), b"")?;
    }
    Ok(root)
}

#[test]
fn optional_entries_share_the_tree_file_budget() -> Result<(), Box<dyn Error>> {
    let root = tree_fixture(512, "tree src\noptional absent\n")?;
    let result = archive_guard_inputs::fingerprint(&root);
    fs::remove_dir_all(root)?;
    assert!(
        result
            .expect_err("optional entry bypassed aggregate limit")
            .to_string()
            .contains("excessive files")
    );
    Ok(())
}

#[test]
fn source_directory_count_is_bounded() -> Result<(), Box<dyn Error>> {
    let root = tree_fixture(0, "tree src\n")?;
    for index in 0..512 {
        fs::create_dir(root.join("src").join(format!("directory-{index}")))?;
    }
    let result = archive_guard_inputs::fingerprint(&root);
    fs::remove_dir_all(root)?;
    assert!(
        result
            .expect_err("directory limit bypassed")
            .to_string()
            .contains("directory count limit exceeded")
    );
    Ok(())
}

fn shell_admission(root: &Path) -> Result<std::process::Output, Box<dyn Error>> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or("workspace root missing")?;
    Ok(Command::new("bash")
        .args([
            "-c",
            r#"
set -euo pipefail
repository="$1"
fail() { printf '%s\n' "$*" >&2; exit 1; }
check_directory() { [[ -d "$1" && ! -L "$1" ]] || fail 'unsafe directory'; }
source "$2/scripts/check-owned-archive-guard-sources.sh"
validate_source_manifest
"#,
            "source-bound-test",
        ])
        .arg(root)
        .arg(workspace)
        .output()?)
}

#[test]
fn stage_zero_counts_tree_and_following_optional_entries() -> Result<(), Box<dyn Error>> {
    let root = tree_fixture(512, "tree src\noptional absent\n")?;
    let output = shell_admission(&root)?;
    fs::remove_dir_all(root)?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("source file count exceeded"));
    Ok(())
}

#[test]
fn stage_zero_counts_tree_bytes_before_following_files() -> Result<(), Box<dyn Error>> {
    let manifest = "tree src\nfile final\n";
    let root = tree_fixture(0, manifest)?;
    fs::File::create(root.join("src/first"))?.set_len(16 * 1024 * 1024)?;
    let remaining = 16 * 1024 * 1024 - u64::try_from(manifest.len())? - 1;
    fs::File::create(root.join("src/second"))?.set_len(remaining)?;
    fs::write(root.join("final"), b"x")?;
    let accepted = shell_admission(&root)?;
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    fs::write(root.join("final"), b"xx")?;
    let output = shell_admission(&root)?;
    fs::remove_dir_all(root)?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("total size limit"));
    Ok(())
}
