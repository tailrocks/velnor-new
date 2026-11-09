// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::receipt_bytes;
use sha2::{Digest, Sha256};
use std::error::Error;
use velnor_actions_contract::QualificationCacheArtifact;

#[test]
fn rejects_duplicate_receipts_with_valid_archive_digest() -> Result<(), Box<dyn Error>> {
    for (duplicate, bytes, expected_sha, expected_len) in [
        (
            false,
            include_bytes!("../../tests/fixtures/duplicate_zip/receipt-single.zip").as_slice(),
            "8fcab496f1fac2958859a3e0260078b7460330a7681ce33b682ab00bd306e227",
            169,
        ),
        (
            true,
            include_bytes!("../../tests/fixtures/duplicate_zip/receipt-duplicate.zip").as_slice(),
            "129ffaaa2a18232a3c030e7307288e307b51d6bcf86cd9a9d17956fafc363d28",
            316,
        ),
    ] {
        assert_eq!(bytes.len(), expected_len);
        let hash = Sha256::digest(bytes);
        let digest: String = hash
            .iter()
            .flat_map(|byte| {
                const HEX: &[u8; 16] = b"0123456789abcdef";
                [
                    char::from(HEX[usize::from(byte >> 4)]),
                    char::from(HEX[usize::from(byte & 15)]),
                ]
            })
            .collect();
        assert_eq!(digest, expected_sha);
        let artifact = QualificationCacheArtifact {
            id: 1,
            name: "fixture".to_owned(),
            digest: format!("sha256:{digest}"),
            size_bytes: bytes.len() as u64,
            expired: false,
            workflow_run_id: 1,
            workflow_head_branch: "main".to_owned(),
            workflow_head_sha: "a".repeat(40),
        };
        let result = receipt_bytes(bytes, &artifact);
        if duplicate {
            assert!(result.is_err());
        } else {
            assert_eq!(result?, b"receipt");
        }
    }
    Ok(())
}
