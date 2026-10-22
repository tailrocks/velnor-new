use super::{Document, linux_device_number};
use sha2::{Digest, Sha256};

const GOLDEN: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../../.velnor/storage-cache-admission-v1-golden.json"
));

#[test]
fn linux_device_encoding_preserves_extended_minor_and_major_bits() {
    assert_eq!(linux_device_number(0, 48), 48);
    assert_eq!(linux_device_number(0, 256), 1_048_576);
    assert_eq!(
        linux_device_number(0x1234_5678, 0x9abc_def0),
        0x1234_59ab_cde6_78f0
    );
}

#[test]
fn production_document_encoding_matches_consumer_golden() {
    let guest_image = crate::worker::guest_image::test_support::TestGuestImage::new()
        .expect("canonical guest image fixture");
    let guest_image_bytes = guest_image
        .file
        .verified_document_bytes(&guest_image.connection, &guest_image.images)
        .expect("verified guest image bytes");
    let guest_image_sha256 = Sha256::digest(guest_image_bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut value: serde_json::Value =
        serde_json::from_slice(GOLDEN).expect("golden document value");
    value["guest_image_sha256"] = serde_json::Value::String(guest_image_sha256);
    let document: Document = serde_json::from_value(value).expect("golden document");
    let runner_directory_inode = document.job_facts_mount.host_directory_inode;
    let snapshot = document
        .encode("a".repeat(64), runner_directory_inode)
        .expect("valid production document");
    if std::env::var_os("VELNOR_REGENERATE_STORAGE_ADMISSION_GOLDEN").is_some() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../.velnor/storage-cache-admission-v1-golden.json");
        std::fs::write(path, snapshot.bytes()).expect("regenerate canonical consumer golden");
        return;
    }
    assert_eq!(snapshot.bytes(), GOLDEN);
}
