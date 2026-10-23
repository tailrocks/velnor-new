use super::*;
use sha2::Digest;
use std::{
    fs,
    io::{Cursor, Write},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use zip::write::SimpleFileOptions;

const CHILD_MARKER: &str = "VELNOR_SOURCE_ADMISSION_FINALIZE_CHILD";
const TEST_NAME: &str = "package_release_publish::release_policy_admission::source::finalize::tests::source_admission_upload_mode_survives_restrictive_umask";

#[test]
fn source_admission_upload_mode_survives_restrictive_umask() {
    if std::env::var_os(CHILD_MARKER).is_some() {
        child_write_and_validate();
        return;
    }

    let root = tempfile::tempdir().expect("runner temp");
    let runner_temp = root.path().canonicalize().expect("absolute runner temp");
    let executable = std::env::current_exe().expect("test executable");
    let output = Command::new("/bin/sh")
        .env_clear()
        .env(CHILD_MARKER, "1")
        .env("RUNNER_TEMP", &runner_temp)
        .args([
            "-c",
            "umask 077; exec \"$1\" --exact \"$2\" --nocapture",
            "sh",
        ])
        .arg(executable)
        .arg(TEST_NAME)
        .output()
        .expect("isolated child process");
    assert!(
        output.status.success(),
        "child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        runner_temp.join("child-executed").is_file(),
        "exact child test ran and completed its assertions; stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
}

fn child_write_and_validate() {
    let admission = br#"{"schema":"velnor.preview-source-admission.v1"}"#;
    let policy = b"private release policy";
    let plan = b"private release plan";
    write_finalization_files(
        admission,
        policy,
        plan,
        CheckDeadline::after(Duration::from_secs(5)).expect("deadline"),
    )
    .expect("finalization outputs");

    let root = runner_root().expect("runner root");
    let admission_path = root.join(ADMISSION_FILE);
    let metadata = fs::symlink_metadata(&admission_path).expect("admission metadata");
    assert_eq!(metadata.mode() & 0o7777, 0o644);
    assert_eq!(
        fs::read(&admission_path).expect("admission bytes"),
        admission
    );
    for (name, bytes) in [
        ("package-release-policy.json", policy.as_slice()),
        ("package-release-plan.json", plan.as_slice()),
    ] {
        let path = root.join(name);
        assert_eq!(fs::read(&path).expect("private file bytes"), bytes);
        assert_eq!(
            fs::symlink_metadata(path)
                .expect("private file metadata")
                .mode()
                & 0o7777,
            0o600
        );
    }

    validate_upload_archive(&root, admission, metadata.mode() & 0o7777);
    fs::write(
        PathBuf::from(std::env::var_os("RUNNER_TEMP").expect("RUNNER_TEMP")).join("child-executed"),
        b"completed",
    )
    .expect("child marker");
}

fn validate_upload_archive(root: &Path, admission: &[u8], mode: u32) {
    let digest = digest_hex(admission);
    let zip_path = root.join("admission.zip");
    let zip_bytes = zip_with_mode(admission, mode);
    fs::write(&zip_path, &zip_bytes).expect("write test ZIP");
    let destination = root.join("accepted");
    fs::create_dir(&destination).expect("empty destination");
    let mut archive = fs::File::open(&zip_path).expect("open test ZIP");
    crate::package_release_publish::artifact_zip::extract_file(
        &mut archive,
        crate::package_release_publish::artifact_zip::ZipProfile::PreviewAggregate,
        &destination,
        &[expected_entry(
            digest.clone(),
            admission.len() as u64,
            0o644,
        )],
        CheckDeadline::after(Duration::from_secs(5)).expect("deadline"),
    )
    .expect("exact 0644 admission ZIP");
    assert_eq!(
        fs::read(destination.join(ADMISSION_FILE)).expect("extracted"),
        admission
    );

    let mutated = mutate_zip_mode(zip_bytes, 0o600);
    let mut parsed = zip::ZipArchive::new(Cursor::new(&mutated)).expect("mutated ZIP");
    assert_eq!(
        parsed
            .by_name(ADMISSION_FILE)
            .expect("mutated admission entry")
            .unix_mode(),
        Some(0o100_600),
        "the permission mutation preserves regular-file type"
    );
    fs::write(&zip_path, mutated).expect("write mutated test ZIP");
    let mut archive = fs::File::open(&zip_path).expect("open mutated ZIP");
    let rejected = root.join("rejected");
    fs::create_dir(&rejected).expect("empty rejected destination");
    assert!(
        crate::package_release_publish::artifact_zip::extract_file(
            &mut archive,
            crate::package_release_publish::artifact_zip::ZipProfile::PreviewAggregate,
            &rejected,
            &[expected_entry(digest, admission.len() as u64, 0o644)],
            CheckDeadline::after(Duration::from_secs(5)).expect("deadline"),
        )
        .is_err(),
        "mutated 0600 metadata must fail the strict artifact reader"
    );
}

fn digest_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    sha2::Sha256::digest(bytes)
        .iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 0x0f)]),
            ]
        })
        .collect()
}

fn expected_entry(
    digest: String,
    size: u64,
    mode: u32,
) -> crate::package_release_publish::artifact_zip::ExpectedZipEntry {
    crate::package_release_publish::artifact_zip::ExpectedZipEntry {
        name: ADMISSION_FILE.to_owned(),
        size: Some(size),
        sha256: Some(digest),
        max_bytes: MAX_ADMISSION_BYTES as u64,
        unix_permissions: Some(mode),
    }
}

fn zip_with_mode(bytes: &[u8], mode: u32) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file(
            ADMISSION_FILE,
            SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored)
                .unix_permissions(mode),
        )
        .expect("ZIP entry");
    writer.write_all(bytes).expect("ZIP body");
    writer.finish().expect("ZIP finish").into_inner()
}

fn mutate_zip_mode(mut bytes: Vec<u8>, mode: u32) -> Vec<u8> {
    let central = bytes
        .windows(4)
        .position(|signature| signature == b"PK\x01\x02")
        .expect("central directory entry");
    let attributes = u32::from_le_bytes([
        bytes[central + 38],
        bytes[central + 39],
        bytes[central + 40],
        bytes[central + 41],
    ]);
    let permissions = (mode & 0o7777) << 16;
    bytes[central + 38..central + 42]
        .copy_from_slice(&((attributes & !(0o7777 << 16)) | permissions).to_le_bytes());
    bytes
}
