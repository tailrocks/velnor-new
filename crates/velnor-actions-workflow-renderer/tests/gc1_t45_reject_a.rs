// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

pub(crate) fn check_rejects_record_shapes(
    fixture: &Fixture,
    record: &str,
) -> Result<(), Box<dyn Error>> {
    expect_record_rejection(
        fixture,
        "dup.json",
        "{\"schema\":1,\"schema\":2}",
        Some("RECORD-REJECTED Duplicate JSON key: schema"),
    )?;
    expect_record_rejection(
        fixture,
        "constant.json",
        &replace_once(record, "\"velnor.job-image-candidate.v1\"", "NaN")?,
        Some("JSON constant has an incorrect value"),
    )?;
    expect_record_rejection(
        fixture,
        "unknown.json",
        &replace_once(record, "{\"schema\"", "{\"unknown_field\":1,\"schema\"")?,
        Some("Candidate record has unknown or missing fields."),
    )?;
    expect_record_rejection(
        fixture,
        "missing.json",
        &replace_once(record, "\"platform\":\"linux/amd64\",", "")?,
        Some("Candidate record has unknown or missing fields."),
    )?;
    expect_record_rejection(
        fixture,
        "schema.json",
        &replace_once(record, "velnor.job-image-candidate.v1", "other.schema.v1")?,
        Some("Candidate schema or platform is incorrect."),
    )?;
    Ok(())
}

pub(crate) fn check_rejects_record_identities(
    fixture: &Fixture,
    record: &str,
) -> Result<(), Box<dyn Error>> {
    expect_record_rejection(
        fixture,
        "commit.json",
        &replace_once(record, &"0".repeat(40), "xyz")?,
        Some("Source identity is incorrect."),
    )?;
    expect_record_rejection(
        fixture,
        "ref.json",
        &replace_once(record, "refs/heads/main", "main")?,
        Some("Source identity is incorrect."),
    )?;
    expect_record_rejection(
        fixture,
        "inputs.json",
        &replace_once(
            record,
            "\".velnor/runner.lock\":\"0000000000000000000000000000000000000000000000000000000000000000\",",
            "",
        )?,
        Some("The source input list is incomplete or unknown."),
    )?;
    expect_record_rejection(
        fixture,
        "repository.json",
        &replace_once(record, "\"repository_id\":1", "\"repository_id\":true")?,
        Some("Workflow repository ID is incorrect."),
    )?;
    expect_record_rejection(
        fixture,
        "authority.json",
        &replace_once(
            record,
            &format!("\"authority_commit\":\"{}\"", "1".repeat(40)),
            "\"authority_commit\":\"not-a-commit\"",
        )?,
        Some("Workflow authority commit is incorrect."),
    )?;
    expect_record_rejection(
        fixture,
        "arguments.json",
        &replace_once(record, "\"arguments\":[]", "\"arguments\":[\"--x\"]")?,
        Some("Build arguments must be empty."),
    )?;
    Ok(())
}

pub(crate) fn check_rejects_record_digests(
    fixture: &Fixture,
    record: &str,
    parts: &ImageParts,
    archive: &[u8],
) -> Result<(), Box<dyn Error>> {
    expect_record_rejection(
        fixture,
        "digest.json",
        &replace_once(
            record,
            &format!("\"sha256\":\"{}\"", sha256_hex(archive)),
            &format!("\"sha256\":\"A{}\"", "0".repeat(63)),
        )?,
        Some("Archive identity is incorrect."),
    )?;
    let layer_digest = format!("\"sha256:{}\"", sha256_hex(&parts.layer));
    expect_record_rejection(
        fixture,
        "layers.json",
        &replace_once(
            record,
            &format!("\"layer_digests\":[{layer_digest}]"),
            "\"layer_digests\":[]",
        )?,
        Some("OCI layer digest list is incorrect."),
    )?;
    let oversized = vec![b' '; 1024 * 1024 + 1];
    let path = write_bytes(fixture.root.path(), "oversized.json", &oversized)?;
    let (code, output) = run_driver(&fixture.driver, fixture.root.path(), "record_bad", &path)?;
    if code != 0 {
        return Err(fail(&format!(
            "fixed record validator left its size bound:{output}"
        )));
    }
    require(&output, "RECORD-REJECTED", "oversized record")?;
    require(&output, "exceeds its size limit.", "oversized record")?;
    Ok(())
}

pub(crate) fn check_rejects_bad_archives(
    fixture: &Fixture,
    parts: &ImageParts,
    archive: &[u8],
) -> Result<(), Box<dyn Error>> {
    expect_archive_rejection(
        fixture,
        "garbage.tar",
        b"not-a-tar-archive",
        Some("Cannot read OCI archive."),
    )?;
    expect_archive_rejection(
        fixture,
        "compressed.tar",
        b"\x1f\x8bgzip-bytes-are-not-a-tar",
        Some("OCI archive must use an uncompressed tar format."),
    )?;
    let mut without_index = Vec::new();
    for (name, data) in [
        ("oci-layout", parts.layout.as_slice()),
        (
            format!("blobs/sha256/{}", sha256_hex(&parts.manifest)).as_str(),
            parts.manifest.as_slice(),
        ),
    ] {
        without_index.extend_from_slice(&tar_entry(name, data, b'0'));
    }
    without_index.extend_from_slice(&[0u8; 1024]);
    expect_archive_rejection(
        fixture,
        "noindex.tar",
        &without_index,
        Some("OCI archive must contain oci-layout and index.json."),
    )?;
    expect_archive_rejection(
        fixture,
        "traversal.tar",
        &splice_before_end(archive.to_vec(), &tar_entry("../evil", b"x", b'0')),
        Some("OCI archive contains a path that is not safe."),
    )?;
    expect_archive_rejection(
        fixture,
        "absolute.tar",
        &splice_before_end(archive.to_vec(), &tar_entry("/evil", b"x", b'0')),
        Some("OCI archive contains a path that is not safe."),
    )?;
    expect_archive_rejection(
        fixture,
        "duplicate.tar",
        &splice_before_end(
            archive.to_vec(),
            &tar_entry("oci-layout", &parts.layout, b'0'),
        ),
        Some("Duplicate archive path: oci-layout"),
    )?;
    Ok(())
}
