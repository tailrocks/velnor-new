use super::*;

use std::io::{Cursor, Write};

use velnor_actions_contract_config::{ArtifactBuildOutput, ArtifactBuildTask, VerificationRunner};
use velnor_actions_contract_workflow::{ArtifactBuildIdentity, ArtifactBuildProvider};

fn task() -> ArtifactBuildTask {
    ArtifactBuildTask {
        id: "bundle".to_owned(),
        mise_task: "build-bundle".to_owned(),
        runner: VerificationRunner::LinuxX64,
        timeout_minutes: 15,
        outputs: vec![ArtifactBuildOutput {
            id: "archive".to_owned(),
            path: "dist/archive.tar".to_owned(),
            max_bytes: 64,
        }],
    }
}

fn expectation(task: &ArtifactBuildTask) -> ArtifactBuildExpectation {
    let identity = ArtifactBuildIdentity {
        repository_id: "123".to_owned(),
        repository: "owner/repo".to_owned(),
        source_sha: "a".repeat(40),
        plan_digest: format!("b3-{}", "0".repeat(64)),
        run_id: "7".to_owned(),
        run_attempt: 1,
        workflow_job_id: "artifact-build".to_owned(),
        provider: ArtifactBuildProvider::GithubHosted,
        task_id: task.id.clone(),
    };
    ArtifactBuildExpectation {
        identity,
        outputs: task.outputs.clone(),
    }
}

fn populate(root: &Path, task: &ArtifactBuildTask, expected: &ArtifactBuildExpectation) {
    fs::create_dir_all(root.join(ARTIFACT_BUILD_OUTPUTS_DIRECTORY)).expect("output directory");
    let bytes = b"artifact contents";
    fs::write(
        root.join(ARTIFACT_BUILD_OUTPUTS_DIRECTORY)
            .join("archive.bin"),
        bytes,
    )
    .expect("output bytes");
    let result = velnor_actions_contract_workflow::export_artifact_result(
        expected.identity.clone(),
        task,
        &[("archive".to_owned(), bytes.to_vec())],
    )
    .expect("result manifest");
    fs::write(
        root.join(ARTIFACT_BUILD_RESULT_FILENAME),
        serde_json::to_vec(&result).expect("json result"),
    )
    .expect("write result");
}

fn archive(entries: &[(&str, &[u8], u32)], compressed: bool) -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    let method = if compressed {
        zip::CompressionMethod::Deflated
    } else {
        zip::CompressionMethod::Stored
    };
    for (name, bytes, mode) in entries {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(method)
            .unix_permissions(*mode);
        if *mode & 0o170_000 == 0o120_000 {
            writer
                .add_symlink(
                    *name,
                    std::str::from_utf8(bytes).expect("symlink target is UTF-8"),
                    options,
                )
                .expect("start archive symlink");
        } else if name.ends_with('/') {
            writer
                .add_directory(*name, options)
                .expect("start archive directory");
        } else {
            writer
                .start_file(*name, options)
                .expect("start archive entry");
            writer.write_all(bytes).expect("write archive entry");
        }
    }
    writer.finish().expect("finish archive").into_inner()
}

fn result_bytes(
    task: &ArtifactBuildTask,
    expected: &ArtifactBuildExpectation,
    output: &[u8],
) -> Vec<u8> {
    let result = velnor_actions_contract_workflow::export_artifact_result(
        expected.identity.clone(),
        task,
        &[("archive".to_owned(), output.to_vec())],
    )
    .expect("valid artifact result");
    serde_json::to_vec(&result).expect("serialize result")
}

#[test]
fn exact_artifact_tree_is_stream_hashed_and_bound_to_result() {
    let task = task();
    let expected = expectation(&task);
    let temp = tempfile::tempdir().expect("tempdir");
    populate(temp.path(), &task, &expected);
    let (result, outputs) = inspect_download(&expected, &task, temp.path()).expect("valid tree");
    assert_eq!(result.identity, expected.identity);
    assert_eq!(outputs.len(), 1);
    assert_eq!(outputs[0].size_bytes, 17);
    assert_eq!(
        outputs[0].digest,
        velnor_actions_contract::digest_b3(b"artifact contents")
    );
}

#[test]
fn download_rejects_extra_files_symlinks_and_oversize_outputs() {
    let task = task();
    let expected = expectation(&task);
    let extra = tempfile::tempdir().expect("tempdir");
    populate(extra.path(), &task, &expected);
    fs::write(extra.path().join("unexpected"), b"x").expect("extra file");
    assert_eq!(
        inspect_download(&expected, &task, extra.path()).expect_err("extra entry"),
        "artifact_entry_inventory_mismatch"
    );

    let symlink = tempfile::tempdir().expect("tempdir");
    populate(symlink.path(), &task, &expected);
    let output = symlink
        .path()
        .join(ARTIFACT_BUILD_OUTPUTS_DIRECTORY)
        .join("archive.bin");
    fs::remove_file(&output).expect("remove output");
    #[cfg(unix)]
    std::os::unix::fs::symlink("/etc/passwd", &output).expect("symlink output");
    #[cfg(unix)]
    assert!(inspect_download(&expected, &task, symlink.path()).is_err());

    let oversized = tempfile::tempdir().expect("tempdir");
    populate(oversized.path(), &task, &expected);
    fs::write(
        oversized
            .path()
            .join(ARTIFACT_BUILD_OUTPUTS_DIRECTORY)
            .join("archive.bin"),
        [b'x'; 65],
    )
    .expect("oversize output");
    assert_eq!(
        inspect_download(&expected, &task, oversized.path()).expect_err("over bound"),
        "artifact_output_oversize"
    );
}

#[test]
fn download_destinations_are_new_and_parent_symlinks_are_rejected() {
    let temp = tempfile::tempdir().expect("tempdir");
    let run = temp.path().join("run");
    fs::create_dir(&run).expect("run dir");
    let path = create_download_dir(&run, "artifact-7-a1").expect("new destination");
    assert!(path.is_dir());
    assert_eq!(
        create_download_dir(&run, "artifact-7-a1").expect_err("never reuse a prior download"),
        "artifact_download_destination_exists"
    );

    let outside = temp.path().join("outside");
    fs::create_dir(&outside).expect("outside");
    let symlink_parent = temp.path().join("symlink-run");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &symlink_parent).expect("parent symlink");
    #[cfg(unix)]
    assert_eq!(
        create_download_dir(&symlink_parent, "artifact-8-a1").expect_err("symlink parent refused"),
        "artifact_directory_not_regular"
    );
}

#[test]
fn archive_preflights_exact_members_then_extracts_a_valid_result() {
    let task = task();
    let expected = expectation(&task);
    let output = b"official task output";
    let result = result_bytes(&task, &expected, output);
    let bytes = archive(
        &[
            (ARTIFACT_BUILD_RESULT_FILENAME, &result, 0o100_644),
            ("outputs/", b"", 0o040_755),
            ("outputs/archive.bin", output, 0o100_644),
        ],
        true,
    );
    let destination = tempfile::tempdir().expect("destination");
    extract_artifact_archive(
        Cursor::new(bytes.as_slice()),
        u64::try_from(bytes.len()).expect("archive size"),
        4096,
        &expected,
        &task,
        destination.path(),
    )
    .expect("bounded archive extracted");
    let (stored, outputs) = inspect_download(&expected, &task, destination.path())
        .expect("exact extracted tree validates");
    assert_eq!(stored.identity, expected.identity);
    assert_eq!(outputs[0].size_bytes, output.len() as u64);
    assert_eq!(
        outputs[0].digest,
        velnor_actions_contract::digest_b3(output)
    );
}

#[test]
fn compressed_archive_expansion_is_rejected_before_any_extraction() {
    let task = task();
    let expected = expectation(&task);
    let expanded_output = vec![b'x'; 65_536];
    let result = result_bytes(&task, &expected, b"small");
    let bytes = archive(
        &[
            (ARTIFACT_BUILD_RESULT_FILENAME, &result, 0o100_644),
            ("outputs/archive.bin", &expanded_output, 0o100_644),
        ],
        true,
    );
    assert!(
        bytes.len() < 4096,
        "fixture must be much smaller than its expansion"
    );
    let destination = tempfile::tempdir().expect("destination");
    let error = extract_artifact_archive(
        Cursor::new(bytes.as_slice()),
        u64::try_from(bytes.len()).expect("archive size"),
        4096,
        &expected,
        &task,
        destination.path(),
    )
    .expect_err("declared expansion exceeds output bound");
    assert_eq!(error, "artifact_archive_member_size_invalid");
    assert!(
        !destination
            .path()
            .join(ARTIFACT_BUILD_RESULT_FILENAME)
            .exists()
    );
    assert!(
        !destination
            .path()
            .join(ARTIFACT_BUILD_OUTPUTS_DIRECTORY)
            .exists()
    );
}

fn assert_archive_rejected_without_output(
    bytes: &[u8],
    max_archive_bytes: u64,
    expected_error: &str,
) {
    let task = task();
    let expected = expectation(&task);
    let destination = tempfile::tempdir().expect("destination");
    assert_eq!(
        extract_artifact_archive(
            Cursor::new(bytes),
            u64::try_from(bytes.len()).expect("archive size"),
            max_archive_bytes,
            &expected,
            &task,
            destination.path()
        )
        .expect_err("archive rejected"),
        expected_error
    );
    assert!(
        !destination
            .path()
            .join(ARTIFACT_BUILD_OUTPUTS_DIRECTORY)
            .exists()
    );
}

fn duplicate_member_archive(result: &[u8]) -> Vec<u8> {
    let mut duplicate = archive(
        &[
            (ARTIFACT_BUILD_RESULT_FILENAME, result, 0o100_644),
            ("outputs/archive.bin", b"one", 0o100_644),
            ("outputs/archive.bim", b"two", 0o100_644),
        ],
        false,
    );
    let unique_name = b"outputs/archive.bim";
    let duplicate_name = b"outputs/archive.bin";
    assert_eq!(unique_name.len(), duplicate_name.len());
    let mut offset = 0;
    let mut replacements = 0;
    while let Some(relative) = duplicate[offset..]
        .windows(unique_name.len())
        .position(|window| window == unique_name)
    {
        let start = offset + relative;
        duplicate[start..start + unique_name.len()].copy_from_slice(duplicate_name);
        offset = start + unique_name.len();
        replacements += 1;
    }
    assert_eq!(
        replacements, 2,
        "local and central ZIP names were rewritten"
    );
    duplicate
}

#[test]
fn archive_rejects_symlink_member_before_writing_outputs() {
    let task = task();
    let expected = expectation(&task);
    let result = result_bytes(&task, &expected, b"small");
    let symlink = archive(
        &[
            (ARTIFACT_BUILD_RESULT_FILENAME, &result, 0o100_644),
            ("outputs/archive.bin", b"../target", 0o120_777),
        ],
        false,
    );
    assert_archive_rejected_without_output(&symlink, 4096, "artifact_archive_nonregular_member");
}

#[test]
fn archive_rejects_exact_duplicate_raw_members_before_writing_outputs() {
    let task = task();
    let expected = expectation(&task);
    let result = result_bytes(&task, &expected, b"small");
    let duplicate = duplicate_member_archive(&result);
    assert_eq!(
        central_entry_count(
            &mut Cursor::new(duplicate.as_slice()),
            duplicate.len() as u64
        )
        .expect("duplicate central-directory count"),
        3
    );
    let duplicate_archive = zip::ZipArchive::new(Cursor::new(duplicate.as_slice()))
        .expect("duplicate ZIP fixture parses");
    assert_eq!(
        duplicate_archive.len(),
        2,
        "ZipArchive's filename map hides exact duplicate raw names"
    );
    assert_archive_rejected_without_output(&duplicate, 4096, "artifact_archive_duplicate_member");
}

#[test]
fn archive_rejects_unexpected_paths_before_writing_outputs() {
    let task = task();
    let expected = expectation(&task);
    let result = result_bytes(&task, &expected, b"small");
    let unexpected = archive(
        &[
            (ARTIFACT_BUILD_RESULT_FILENAME, &result, 0o100_644),
            ("outputs/../escape", b"x", 0o100_644),
        ],
        false,
    );
    assert_archive_rejected_without_output(
        &unexpected,
        4096,
        "artifact_archive_inventory_mismatch",
    );
}

#[test]
fn archive_rejects_compressed_bytes_over_the_declared_cap() {
    let task = task();
    let expected = expectation(&task);
    let result = result_bytes(&task, &expected, b"small");
    let valid = archive(
        &[
            (ARTIFACT_BUILD_RESULT_FILENAME, &result, 0o100_644),
            ("outputs/archive.bin", b"small", 0o100_644),
        ],
        false,
    );
    assert_archive_rejected_without_output(&valid, 1, "artifact_archive_size_invalid");
}
