use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use flate2::Compression;
use flate2::write::GzEncoder;
use tar::{Builder, EntryType, Header};

use super::{
    Pin, archive_name, export_archive, import_archive, parse_pins, prepare_handoff, verify_marker,
};

type TestResult = Result<(), Box<dyn Error>>;

fn pin() -> Pin {
    Pin {
        id: "asset-one".to_owned(),
        sha256: "a".repeat(64),
    }
}

fn marker() -> &'static str {
    r#"{
  "schema": 1,
  "sources": [{
    "kind": "release-assets",
    "id": "asset-one",
    "path": "assets/release",
    "repository": "owner/repo",
    "tag": "v1.0.0",
    "commit": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "asset": "release.tar.gz",
    "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  }]
}"#
}

fn workspace() -> Result<(tempfile::TempDir, PathBuf), Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("workspace");
    fs::create_dir_all(root.join(".velnor/deps/assets/release/bin"))?;
    fs::create_dir_all(root.join(".velnor/deps/assets/release/lib"))?;
    fs::create_dir_all(root.join(".velnor/deps/nested"))?;
    fs::write(root.join(".velnor/deps/verified.json"), marker())?;
    fs::write(
        root.join(".velnor/deps/assets/release/lib/runner"),
        b"executable",
    )?;
    fs::write(
        root.join(".velnor/deps/assets/release/lib/private.data"),
        b"private",
    )?;
    fs::write(
        root.join(".velnor/deps/nested/velnor-assets.sha256"),
        b"ordinary nested payload",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        fs::set_permissions(
            root.join(".velnor/deps/assets/release/lib/runner"),
            fs::Permissions::from_mode(0o750),
        )?;
        fs::set_permissions(
            root.join(".velnor/deps/assets/release/lib/private.data"),
            fs::Permissions::from_mode(0o600),
        )?;
        fs::set_permissions(
            root.join(".velnor/deps/assets/release/lib"),
            fs::Permissions::from_mode(0o710),
        )?;
        symlink(
            "../lib/runner",
            root.join(".velnor/deps/assets/release/bin/runner"),
        )?;
    }
    Ok((temp, root))
}

fn runner_temp(temp: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let runner = temp.join("runner");
    fs::create_dir_all(runner.join("velnor"))?;
    Ok(runner)
}

#[test]
fn raw_archive_round_trip_preserves_mode_links_and_nested_index_name() -> TestResult {
    let (_source_temp, source) = workspace()?;
    let package = tempfile::tempdir()?;
    let archive = package.path().join("assets.tar.gz");
    let deps = source.join(".velnor/deps");
    let pins = [pin()];
    verify_marker(&deps, &pins)?;
    export_archive(&deps, &archive)?;

    let (_target_temp, target) = workspace()?;
    fs::remove_dir_all(target.join(".velnor/deps"))?;
    fs::create_dir_all(target.join(".velnor"))?;
    let runner = runner_temp(package.path())?;
    let stage = prepare_handoff(&target, &runner, "suite-one")?;
    fs::copy(&archive, stage.join(archive_name("suite-one")))?;
    let restored = import_archive(&target, &stage, "suite-one", &pins)?;

    assert_eq!(
        fs::read(restored.join("nested/velnor-assets.sha256"))?,
        b"ordinary nested payload"
    );
    assert_eq!(
        fs::read(restored.join("assets/release/bin/runner"))?,
        b"executable"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(restored.join("assets/release/lib/runner"))?
                .permissions()
                .mode()
                & 0o777,
            0o750
        );
        assert_eq!(
            fs::metadata(restored.join("assets/release/lib/private.data"))?
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(restored.join("assets/release/lib"))?
                .permissions()
                .mode()
                & 0o777,
            0o710
        );
        assert_eq!(
            fs::read_link(restored.join("assets/release/bin/runner"))?,
            Path::new("../lib/runner")
        );
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn prepare_removes_planted_deps_symlink_without_touching_its_target() -> TestResult {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir()?;
    let root = temp.path().join("workspace");
    let outside = temp.path().join("outside");
    fs::create_dir_all(root.join(".velnor"))?;
    fs::create_dir(&outside)?;
    fs::write(outside.join("sentinel"), b"keep")?;
    symlink(&outside, root.join(".velnor/deps"))?;
    let runner = runner_temp(temp.path())?;

    let stage = prepare_handoff(&root, &runner, "suite-one")?;

    assert!(fs::symlink_metadata(root.join(".velnor/deps")).is_err());
    assert_eq!(fs::read(outside.join("sentinel"))?, b"keep");
    assert_eq!(fs::read_dir(stage)?.count(), 0);
    Ok(())
}

#[test]
fn prepare_removes_stale_assets_directory_before_download() -> TestResult {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("workspace");
    fs::create_dir_all(root.join(".velnor/deps/stale"))?;
    fs::write(root.join(".velnor/deps/stale/file"), b"stale")?;
    let runner = runner_temp(temp.path())?;

    let _stage = prepare_handoff(&root, &runner, "suite-one")?;

    assert!(fs::symlink_metadata(root.join(".velnor/deps")).is_err());
    Ok(())
}

#[test]
fn importer_rejects_extra_downloaded_files() -> TestResult {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("workspace");
    fs::create_dir_all(root.join(".velnor"))?;
    let stage = temp.path().join("stage");
    fs::create_dir(&stage)?;
    fs::write(stage.join(archive_name("suite-one")), b"not an archive")?;
    fs::write(stage.join("unexpected"), b"extra")?;

    assert!(import_archive(&root, &stage, "suite-one", &[pin()]).is_err());
    assert!(fs::symlink_metadata(root.join(".velnor/deps")).is_err());
    Ok(())
}

#[test]
fn marker_must_match_pins_and_rejected_import_cleans_extracted_assets() -> TestResult {
    let (_source_temp, source) = workspace()?;
    let package = tempfile::tempdir()?;
    let archive = package.path().join("assets.tar.gz");
    export_archive(&source.join(".velnor/deps"), &archive)?;

    let (_target_temp, target) = workspace()?;
    fs::remove_dir_all(target.join(".velnor/deps"))?;
    fs::create_dir_all(target.join(".velnor"))?;
    let stage = package.path().join("stage");
    fs::create_dir(&stage)?;
    fs::copy(&archive, stage.join(archive_name("suite-one")))?;
    let wrong = Pin {
        id: "asset-one".to_owned(),
        sha256: "b".repeat(64),
    };

    assert!(import_archive(&target, &stage, "suite-one", &[wrong]).is_err());
    assert!(fs::symlink_metadata(target.join(".velnor/deps")).is_err());
    Ok(())
}

#[test]
fn pin_parser_rejects_duplicate_ids_and_malformed_digests() {
    let duplicate = format!("asset-one:{} asset-one:{}", "a".repeat(64), "b".repeat(64));
    assert!(parse_pins(&duplicate).is_err());
    assert!(parse_pins("asset-one:abc").is_err());
}

#[test]
fn bounded_reader_rejects_traversal_duplicate_and_special_entries() -> TestResult {
    for entries in [
        vec![("../escape", EntryType::Regular, b"bad".as_slice())],
        vec![
            ("duplicate", EntryType::Regular, b"one".as_slice()),
            ("duplicate", EntryType::Regular, b"two".as_slice()),
        ],
        vec![("fifo", EntryType::Fifo, b"".as_slice())],
    ] {
        let temp = tempfile::tempdir()?;
        let root = temp.path().join("workspace");
        fs::create_dir_all(root.join(".velnor"))?;
        let stage = temp.path().join("stage");
        fs::create_dir(&stage)?;
        let archive = stage.join(archive_name("suite-one"));
        write_tar(&archive, &entries)?;
        assert!(import_archive(&root, &stage, "suite-one", &[pin()]).is_err());
        assert!(fs::symlink_metadata(root.join(".velnor/deps")).is_err());
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn bounded_reader_rejects_escaping_symlink_target() -> TestResult {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("workspace");
    fs::create_dir_all(root.join(".velnor"))?;
    let stage = temp.path().join("stage");
    fs::create_dir(&stage)?;
    let archive = stage.join(archive_name("suite-one"));
    write_symlink_tar(&archive, "../../outside")?;

    assert!(import_archive(&root, &stage, "suite-one", &[pin()]).is_err());
    assert!(fs::symlink_metadata(root.join(".velnor/deps")).is_err());
    Ok(())
}

fn write_tar(path: &Path, entries: &[(&str, EntryType, &[u8])]) -> TestResult {
    let file = fs::File::create(path)?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut archive = Builder::new(encoder);
    for (name, kind, data) in entries {
        let mut header = Header::new_gnu();
        header.set_entry_type(*kind);
        header.set_size(u64::try_from(data.len())?);
        header.set_mode(0o644);
        if name.contains("..") {
            let raw = header.as_mut_bytes();
            raw[..100].fill(0);
            raw[..name.len()].copy_from_slice(name.as_bytes());
            header.set_cksum();
            archive.append(&header, *data)?;
            continue;
        }
        header.set_cksum();
        archive.append_data(&mut header, name, *data)?;
    }
    archive.finish()?;
    let encoder = archive.into_inner()?;
    encoder.finish()?;
    Ok(())
}

#[cfg(unix)]
fn write_symlink_tar(path: &Path, target: &str) -> TestResult {
    let file = fs::File::create(path)?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut archive = Builder::new(encoder);
    let mut header = Header::new_gnu();
    header.set_entry_type(EntryType::Symlink);
    header.set_size(0);
    header.set_mode(0o777);
    header.set_link_name(target)?;
    header.set_cksum();
    archive.append_data(&mut header, "link", &[][..])?;
    archive.finish()?;
    archive.into_inner()?.finish()?;
    Ok(())
}
