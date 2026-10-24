use std::error::Error;
use std::fs;
use std::io::Write as _;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{
    Temporary, create_temp_at, identity_from_fd, named_file_matches, open_private_directory,
    publish_files, unlink_matching,
};
use rustix::fs::{self as unix_fs, Mode};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

#[test]
fn refuses_a_temporary_name_replaced_by_a_symlink() -> Result<(), Box<dyn Error>> {
    let root = fixture_root()?;
    let directory = open_private_directory(&root).map_err(std::io::Error::other)?;
    let mut temporary = create_temp_at(&directory).map_err(std::io::Error::other)?;
    temporary.file.write_all(b"expected bytes")?;
    unix_fs::fchmod(&temporary.file, Mode::from_raw_mode(0o644))?;
    temporary.identity = identity_from_fd(&temporary.file).map_err(std::io::Error::other)?;
    let temp_name = temporary.name.to_str()?.to_owned();
    let temp_path = root.join(&temp_name);
    let replacement = root.join("replacement");
    fs::write(&replacement, b"other bytes")?;
    fs::remove_file(&temp_path)?;
    symlink(&replacement, &temp_path)?;

    assert!(!named_file_matches(
        &directory,
        &temporary.name,
        temporary.identity,
        1,
        false,
        true
    ));
    unlink_matching(
        &directory,
        &temporary.name,
        temporary.identity,
        -1,
        false,
        false,
    );
    assert!(fs::symlink_metadata(&temp_path)?.file_type().is_symlink());
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn failed_partial_publication_removes_only_its_files() -> Result<(), Box<dyn Error>> {
    let root = fixture_root()?;
    let directory = open_private_directory(&root).map_err(std::io::Error::other)?;
    let temporary = [b"binary".as_slice(), b"checksum", b"provenance"]
        .into_iter()
        .map(|bytes| make_temporary(&directory, bytes))
        .collect::<Result<Vec<_>, _>>()?;

    let result = publish_files(
        &directory,
        ["jackin", "SHA256SUMS", "provenance.json"],
        &temporary,
        |published| {
            if published == 1 {
                Err("injected failure after first publication".to_owned())
            } else {
                Ok(())
            }
        },
    );
    assert!(result.is_err());
    assert_eq!(fs::read_dir(&root)?.count(), 0);
    fs::remove_dir_all(root)?;
    Ok(())
}

#[test]
fn rejects_shared_writable_ancestor_and_output_directory() -> Result<(), Box<dyn Error>> {
    let root = fixture_root()?;
    let child = root.join("candidate");
    fs::create_dir(&child)?;
    fs::set_permissions(&root, fs::Permissions::from_mode(0o777))?;
    assert!(open_private_directory(&child).is_err());
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    fs::set_permissions(&child, fs::Permissions::from_mode(0o777))?;
    assert!(open_private_directory(&child).is_err());
    fs::set_permissions(&child, fs::Permissions::from_mode(0o700))?;
    let link = root.join("candidate-link");
    symlink(&child, &link)?;
    assert!(open_private_directory(&link).is_err());
    fs::remove_dir_all(root)?;
    Ok(())
}

fn make_temporary(directory: &fs::File, content: &[u8]) -> Result<Temporary, Box<dyn Error>> {
    use std::io::Write as _;

    let mut temporary = create_temp_at(directory).map_err(std::io::Error::other)?;
    temporary.file.write_all(content)?;
    unix_fs::fchmod(&temporary.file, Mode::from_raw_mode(0o644))?;
    temporary.file.sync_all()?;
    temporary.identity = identity_from_fd(&temporary.file).map_err(std::io::Error::other)?;
    Ok(temporary)
}

fn fixture_root() -> Result<PathBuf, Box<dyn Error>> {
    let id = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "velnor-candidate-extract-{}-{id}",
        std::process::id()
    ));
    fs::create_dir(&path)?;
    Ok(path)
}
