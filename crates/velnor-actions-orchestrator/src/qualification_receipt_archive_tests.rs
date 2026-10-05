use std::error::Error;
use std::io::{Cursor, Write};

use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::{RECEIPT_FILENAME, verify_and_extract};

#[test]
fn accepts_only_the_digest_bound_receipt_document() -> Result<(), Box<dyn Error>> {
    let payload = br#"{"schema":1}"#;
    let archive = make_archive(&[(RECEIPT_FILENAME, payload, 0o100644)])?;
    let digest = format!("sha256:{:x}", Sha256::digest(&archive));

    assert_eq!(verify_and_extract(&archive, &digest)?, payload);
    Ok(())
}

#[test]
fn rejects_digest_mismatch_before_zip_parsing() -> Result<(), Box<dyn Error>> {
    let archive = make_archive(&[(RECEIPT_FILENAME, b"{}", 0o100644)])?;

    assert!(verify_and_extract(&archive, &format!("sha256:{}", "0".repeat(64))).is_err());
    Ok(())
}

#[test]
fn rejects_unexpected_and_multiple_members() -> Result<(), Box<dyn Error>> {
    let unexpected = make_archive(&[("other.json", b"{}", 0o100644)])?;
    let multiple = make_archive(&[
        (RECEIPT_FILENAME, b"{}", 0o100644),
        ("other.json", b"{}", 0o100644),
    ])?;

    assert!(verify_archive(&unexpected).is_err());
    assert!(verify_archive(&multiple).is_err());
    Ok(())
}

#[test]
fn rejects_symlink_receipt_members() -> Result<(), Box<dyn Error>> {
    let archive = make_archive(&[(RECEIPT_FILENAME, b"{}", 0o120777)])?;

    assert!(verify_archive(&archive).is_err());
    Ok(())
}

fn verify_archive(archive: &[u8]) -> Result<Vec<u8>, crate::OrchestratorError> {
    let digest = format!("sha256:{:x}", Sha256::digest(archive));
    verify_and_extract(archive, &digest)
}

fn make_archive(members: &[(&str, &[u8], u32)]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, payload, mode) in members {
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(*mode);
        writer.start_file(*name, options)?;
        writer.write_all(payload)?;
    }
    Ok(writer.finish()?.into_inner())
}
