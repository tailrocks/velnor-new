use super::super::compression::{ArchiveCompression, encode_archive};
use super::{
    BLOCK, Operation, StageAccess, create_archive, open_archive, parse_args, put_field, put_octal,
    scan_archive, write_single_member,
};
use crate::cache_action_stage::{
    CacheActionCompression, StageContext, create_stage, open_stage, run_cache_tar,
};
use std::ffi::OsString;
use std::io::{Seek, SeekFrom, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn member(path: &str, kind: u8, body: &[u8]) -> Vec<u8> {
    let mut header = [0_u8; BLOCK];
    put_field(&mut header[..100], path.as_bytes()).expect("test path fits");
    put_octal(&mut header[100..108], 0o600).expect("mode fits");
    put_octal(&mut header[108..116], 0).expect("uid fits");
    put_octal(&mut header[116..124], 0).expect("gid fits");
    put_octal(&mut header[124..136], body.len() as u64).expect("size fits");
    put_octal(&mut header[136..148], 0).expect("mtime fits");
    header[148..156].fill(b' ');
    header[156] = kind;
    header[257..263].copy_from_slice(b"ustar\0");
    header[263..265].copy_from_slice(b"00");
    let checksum = header.iter().map(|byte| u64::from(*byte)).sum::<u64>();
    let checksum = format!("{checksum:06o}");
    header[148..154].copy_from_slice(checksum.as_bytes());
    header[154] = 0;
    header[155] = b' ';

    let mut archive = header.to_vec();
    archive.extend_from_slice(body);
    archive.resize(archive.len().div_ceil(BLOCK) * BLOCK, 0);
    archive
}

fn finish(mut archive: Vec<u8>) -> std::fs::File {
    archive.extend_from_slice(&[0; BLOCK * 2]);
    let mut file = tempfile::tempfile().expect("temporary archive");
    file.write_all(&archive).expect("write archive");
    file.seek(SeekFrom::Start(0)).expect("rewind archive");
    file
}

fn archive_path(runner_temp: &Path) -> std::path::PathBuf {
    runner_temp.join("cache.tar")
}

#[test]
fn archive_parent_accepts_a_sticky_shared_temporary_ancestor() {
    let root = tempfile::tempdir().expect("temporary root");
    let shared = root.path().join("shared-temp");
    std::fs::create_dir(&shared).expect("create shared temp");
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o1777))
        .expect("protect shared temp with sticky bit");
    let runner_temp_path = shared.join("runner-temp");
    std::fs::create_dir(&runner_temp_path).expect("create private runner temp");
    std::fs::set_permissions(&runner_temp_path, std::fs::Permissions::from_mode(0o700))
        .expect("protect runner temp");
    let runner_temp = std::fs::canonicalize(runner_temp_path).expect("canonical runner temp");
    let archive = archive_path(&runner_temp);
    let resolved = super::resolve_under(&runner_temp, archive.as_os_str())
        .expect("resolve archive under runner temp");
    super::super::stage::open_parent(&resolved).expect("open protected archive parent");

    let (file, _parent, _name) = create_archive(&runner_temp, archive.as_os_str())
        .expect("create archive under sticky shared ancestor");
    drop(file);
    assert_eq!(
        std::fs::metadata(&archive)
            .expect("read created archive metadata")
            .permissions()
            .mode()
            & 0o7777,
        0o600
    );
    open_archive(&runner_temp, archive_path(&runner_temp).as_os_str())
        .expect("open archive under sticky shared ancestor");
}

#[test]
fn archive_path_alias_resolves_under_the_trusted_root_before_descriptor_checks() {
    let root = tempfile::tempdir().expect("temporary root");
    let runner_temp_path = root.path().join("runner-temp");
    std::fs::create_dir(&runner_temp_path).expect("create runner temp");
    std::fs::set_permissions(&runner_temp_path, std::fs::Permissions::from_mode(0o700))
        .expect("protect runner temp");
    let runner_temp = std::fs::canonicalize(runner_temp_path).expect("canonical runner temp");
    let alias = root.path().join("runner-temp-alias");
    std::os::unix::fs::symlink(&runner_temp, &alias).expect("create runner temp alias");
    let archive = alias.join("cache.tar");

    let (file, _parent, _name) = create_archive(&runner_temp, archive.as_os_str())
        .expect("resolve SDK path alias under the trusted root");
    drop(file);
    assert!(runner_temp.join("cache.tar").is_file());
}

#[test]
fn archive_parent_rejects_writable_nonsticky_ancestor_before_creation() {
    let root = tempfile::tempdir().expect("temporary root");
    let shared = root.path().join("shared-temp");
    std::fs::create_dir(&shared).expect("create shared temp");
    std::fs::set_permissions(&shared, std::fs::Permissions::from_mode(0o777))
        .expect("make ancestor unsafe");
    let runner_temp_path = shared.join("runner-temp");
    std::fs::create_dir(&runner_temp_path).expect("create private runner temp");
    std::fs::set_permissions(&runner_temp_path, std::fs::Permissions::from_mode(0o700))
        .expect("protect runner temp");
    let runner_temp = std::fs::canonicalize(runner_temp_path).expect("canonical runner temp");
    let archive = archive_path(&runner_temp);

    assert!(create_archive(&runner_temp, archive.as_os_str()).is_err());
    assert!(open_archive(&runner_temp, archive.as_os_str()).is_err());
    assert!(!archive.exists());
}

#[test]
fn archive_parent_rejects_unsafe_final_permissions() {
    let root = tempfile::tempdir().expect("temporary root");
    let runner_temp_path = root.path().join("runner-temp");
    std::fs::create_dir(&runner_temp_path).expect("create runner temp");
    std::fs::set_permissions(&runner_temp_path, std::fs::Permissions::from_mode(0o777))
        .expect("make final parent unsafe");
    let runner_temp = std::fs::canonicalize(runner_temp_path).expect("canonical runner temp");
    let archive = archive_path(&runner_temp);

    assert!(create_archive(&runner_temp, archive.as_os_str()).is_err());
    assert!(!archive.exists());
}

#[test]
fn archive_parent_rejects_symlink_replacement() {
    let root = tempfile::tempdir().expect("temporary root");
    let runner_temp_path = root.path().join("runner-temp");
    std::fs::create_dir(&runner_temp_path).expect("create runner temp");
    std::fs::set_permissions(&runner_temp_path, std::fs::Permissions::from_mode(0o700))
        .expect("protect runner temp");
    let runner_temp = std::fs::canonicalize(runner_temp_path).expect("canonical runner temp");
    let moved = root.path().join("runner-temp-moved");
    let archive = archive_path(&runner_temp);
    std::fs::rename(&runner_temp, &moved).expect("move runner temp");
    std::os::unix::fs::symlink(&moved, &runner_temp).expect("replace with symlink");

    assert!(create_archive(&runner_temp, archive.as_os_str()).is_err());
    assert!(open_archive(&runner_temp, archive.as_os_str()).is_err());
    assert!(!moved.join("cache.tar").exists());
}

#[test]
fn archive_create_preserves_an_existing_foreign_file() {
    let root = tempfile::tempdir().expect("temporary root");
    let runner_temp_path = root.path().join("runner-temp");
    std::fs::create_dir(&runner_temp_path).expect("create runner temp");
    let runner_temp = std::fs::canonicalize(runner_temp_path).expect("canonical runner temp");
    let archive = archive_path(&runner_temp);
    std::fs::write(&archive, b"foreign archive").expect("write foreign archive");

    assert!(create_archive(&runner_temp, archive.as_os_str()).is_err());
    assert_eq!(
        std::fs::read(archive).expect("read preserved file"),
        b"foreign archive"
    );
}

fn pax_path(path: &str) -> Vec<u8> {
    pax_record("path", path)
}

fn pax_record(key: &str, value: &str) -> Vec<u8> {
    let record = format!("{key}={value}\n");
    for length in record.len()..=record.len() + 16 {
        let line = format!("{length} {record}");
        if line.len() == length {
            return line.into_bytes();
        }
    }
    panic!("PAX record length converges");
}

#[test]
fn accepts_pinned_sdk_tar_arguments_and_rejects_unknown_forms() {
    let create = [
        "--posix",
        "-cf",
        "/tmp/archive.tar",
        "--exclude",
        "/tmp/archive.tar",
        "-P",
        "--delay-directory-restore",
        "-C",
        "/workspace",
        "--files-from",
        "manifest.txt",
        "-z",
    ];
    let create = create.map(OsString::from);
    assert_eq!(
        parse_args(&create)
            .expect("valid SDK create args")
            .operation,
        Operation::Create
    );

    let extract = ["-xf", "/tmp/archive.tar", "-P", "-C", "/workspace", "-z"].map(OsString::from);
    assert_eq!(
        parse_args(&extract)
            .expect("valid SDK extract args")
            .operation,
        Operation::Extract
    );

    let list = ["-tf", "/tmp/archive.tar", "-P", "-z"].map(OsString::from);
    assert_eq!(
        parse_args(&list).expect("valid SDK list args").operation,
        Operation::List
    );

    let long_compressed = [
        "-tf",
        "/tmp/archive.tar",
        "-P",
        "--use-compress-program",
        "unzstd",
        "--long=30",
    ]
    .map(OsString::from);
    assert!(parse_args(&long_compressed).is_ok());

    let compressed_create = [
        "--posix",
        "-cf",
        "/tmp/archive.tar",
        "--exclude",
        "/tmp/archive.tar",
        "-P",
        "-C",
        "/workspace",
        "--files-from",
        "manifest.txt",
        "--use-compress-program",
        "zstdmt",
        "--long=30",
    ]
    .map(OsString::from);
    assert!(parse_args(&compressed_create).is_ok());

    let unsupported =
        ["-xf", "/tmp/archive.tar", "-P", "-C", "/workspace", "-v"].map(OsString::from);
    assert!(parse_args(&unsupported).is_err());
    let missing_compression =
        ["-xf", "/tmp/archive.tar", "-P", "-C", "/workspace"].map(OsString::from);
    assert_eq!(
        parse_args(&missing_compression).map(|_| ()),
        Err("cache_tar_compression_missing")
    );
}

#[test]
fn extraction_without_compression_is_rejected_before_creating_a_stage() {
    let root = tempfile::tempdir().expect("temporary root");
    let workspace = root.path().join("workspace");
    let runner_temp = root.path().join("runner-temp");
    std::fs::create_dir(&workspace).expect("create workspace");
    std::fs::create_dir(&runner_temp).expect("create runner temp");
    let workspace = std::fs::canonicalize(workspace).expect("canonical workspace");
    let runner_temp = std::fs::canonicalize(runner_temp).expect("canonical runner temp");
    let key = format!("mise-tools-v2-payload-v3-{}", "d".repeat(64));
    let context =
        StageContext::normal(&runner_temp, &workspace, &key).expect("cache stage context");
    let source_key = format!("mise-tools-v2-payload-v3-{}", "e".repeat(64));
    let source_context =
        StageContext::normal(&runner_temp, &workspace, &source_key).expect("source context");
    let mut source_writer = create_stage(&source_context).expect("create source stage");
    source_writer
        .write_all(b"valid uncompressed input")
        .expect("write source payload");
    let source_claim = source_writer.finish().expect("finish source stage");
    let mut source = open_stage(&source_context, &source_claim).expect("open source stage");
    let archive_path = archive_path(&runner_temp);
    let mut archive = tempfile::tempfile().expect("create valid raw tar");
    write_single_member(&mut source, &mut archive, context.member_path())
        .expect("write valid one-member tar");
    archive
        .seek(SeekFrom::Start(0))
        .expect("rewind valid raw tar");
    let mut output = std::fs::File::create(&archive_path).expect("create valid archive file");
    std::io::copy(&mut archive, &mut output).expect("write valid uncompressed archive");
    output.sync_all().expect("sync valid archive");

    let args = [
        OsString::from("-xf"),
        archive_path.into_os_string(),
        OsString::from("-P"),
        OsString::from("-C"),
        workspace.into_os_string(),
    ];
    assert_eq!(
        run_cache_tar(&context, &args, StageAccess::New, None).map(|_| ()),
        Err("cache_tar_compression_missing")
    );
    assert!(!context.action_path().exists());
}

#[test]
fn extraction_reports_the_sdk_compression_parsed_from_tar_arguments() {
    let root = tempfile::tempdir().expect("temporary root");
    let workspace = root.path().join("workspace");
    let runner_temp = root.path().join("runner-temp");
    std::fs::create_dir(&workspace).expect("create workspace");
    std::fs::create_dir(&runner_temp).expect("create runner temp");
    let workspace = std::fs::canonicalize(workspace).expect("canonical workspace");
    let runner_temp = std::fs::canonicalize(runner_temp).expect("canonical runner temp");
    let source_key = format!("mise-tools-v2-payload-v3-{}", "b".repeat(64));
    let target_key = format!("mise-tools-v2-payload-v3-{}", "a".repeat(64));
    let source_context =
        StageContext::normal(&runner_temp, &workspace, &source_key).expect("source context");
    let target_context =
        StageContext::normal(&runner_temp, &workspace, &target_key).expect("target context");
    let mut source = create_stage(&source_context).expect("create source stage");
    source
        .write_all(b"verified payload")
        .expect("write source stage");
    let source_claim = source.finish().expect("finish source stage");
    let mut source = open_stage(&source_context, &source_claim).expect("open source stage");
    let mut raw = tempfile::tempfile().expect("temporary tar");
    write_single_member(&mut source, &mut raw, target_context.member_path())
        .expect("write action member");
    raw.seek(SeekFrom::Start(0)).expect("rewind raw tar");
    let archive_path = archive_path(&runner_temp);
    let (mut archive, _parent, _name) =
        create_archive(&runner_temp, archive_path.as_os_str()).expect("create compressed archive");
    encode_archive(raw, &mut archive, ArchiveCompression::Gzip, None).expect("encode gzip archive");
    archive
        .flush()
        .and_then(|()| archive.sync_all())
        .expect("finish compressed archive");

    let args = [
        OsString::from("-xf"),
        archive_path.into_os_string(),
        OsString::from("-P"),
        OsString::from("-C"),
        workspace.into_os_string(),
        OsString::from("-z"),
    ];
    let outcome = run_cache_tar(&target_context, &args, StageAccess::New, None)
        .expect("extract through the trusted tar adapter");

    assert_eq!(outcome.compression, Some(CacheActionCompression::Gzip));
    assert!(outcome.receipt.is_some());
}

#[test]
fn accepts_one_regular_member_with_a_valid_posix_path_record() {
    let expected = "../../runner-temp/stage.payload";
    let mut archive = member("PaxHeaders/stage", b'x', &pax_path(expected));
    archive.extend(member("stage", b'0', b"verified payload"));
    let mut file = finish(archive);
    let mut payload = scan_archive(&mut file, expected).expect("valid one-file archive");
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut payload, &mut bytes).expect("read payload");
    assert_eq!(bytes, b"verified payload");
}

#[test]
fn creates_a_posix_path_record_for_a_long_context_member() {
    let root = tempfile::tempdir().expect("temporary root");
    let workspace = root.path().join("workspace");
    let runner_temp = root.path().join("runner-temp");
    std::fs::create_dir(&workspace).expect("create workspace");
    std::fs::create_dir(&runner_temp).expect("create runner temp");
    let mut nested = runner_temp.clone();
    for index in 0..8 {
        nested.push(format!("nested-{index}-{}", "x".repeat(30)));
        std::fs::create_dir(&nested).expect("create nested stage parent");
    }
    let key = format!("mise-tools-v2-payload-v3-{}", "a".repeat(64));
    let context = StageContext::normal(&nested, &workspace, &key).expect("build stage context");
    assert!(context.member_path().len() > 255);

    let mut writer = create_stage(&context).expect("create stage file");
    writer
        .write_all(b"long path payload")
        .expect("write stage file");
    let claim = writer.finish().expect("finish stage file");
    let mut reader = open_stage(&context, &claim).expect("open verified stage file");
    let mut raw = tempfile::tempfile().expect("temporary tar file");
    write_single_member(&mut reader, &mut raw, context.member_path())
        .expect("write one-file POSIX archive");
    raw.seek(SeekFrom::Start(0)).expect("rewind tar");
    let mut payload =
        scan_archive(&mut raw, context.member_path()).expect("read one-file POSIX archive");
    let mut actual = Vec::new();
    std::io::Read::read_to_end(&mut payload, &mut actual).expect("read payload");
    assert_eq!(actual, b"long path payload");
}

#[test]
fn rejects_foreign_paths_links_multiple_members_and_partial_trailing_data() {
    let expected = "../../runner-temp/stage.payload";
    let mut foreign = finish(member("../../outside", b'0', b"x"));
    assert!(scan_archive(&mut foreign, expected).is_err());

    let mut absolute = finish(member("/outside", b'0', b"x"));
    assert!(scan_archive(&mut absolute, expected).is_err());

    let mut link = finish(member(expected, b'2', b"target"));
    assert!(scan_archive(&mut link, expected).is_err());

    let mut device = finish(member(expected, b'3', b""));
    assert!(scan_archive(&mut device, expected).is_err());

    let mut unsupported_pax = member(
        "PaxHeaders/stage",
        b'x',
        &pax_record("linkpath", "../../outside"),
    );
    unsupported_pax.extend(member("stage", b'0', b"x"));
    let mut unsupported_pax = finish(unsupported_pax);
    assert!(scan_archive(&mut unsupported_pax, expected).is_err());

    let mut two_members = member("first", b'0', b"x");
    two_members.extend(member("second", b'0', b"y"));
    let mut multiple = finish(two_members);
    assert!(scan_archive(&mut multiple, "first").is_err());

    let mut partial = tempfile::tempfile().expect("temporary archive");
    partial
        .write_all(&member(expected, b'0', b"x"))
        .expect("write member");
    partial
        .write_all(&[0; BLOCK * 2])
        .expect("write end markers");
    partial.write_all(&[0]).expect("write incomplete block");
    partial.seek(SeekFrom::Start(0)).expect("rewind archive");
    assert!(scan_archive(&mut partial, expected).is_err());
}
