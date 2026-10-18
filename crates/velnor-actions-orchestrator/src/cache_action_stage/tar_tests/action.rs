use super::super::super::compression::{ArchiveCompression, encode_archive};
use super::super::{
    Operation, StageAccess, create_archive, parse_args, run_cache_tar, write_single_member,
};
use super::common::archive_path;
use crate::cache_action_stage::{CacheActionCompression, StageContext, create_stage, open_stage};
use std::ffi::OsString;
use std::io::{Seek, SeekFrom, Write};

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
