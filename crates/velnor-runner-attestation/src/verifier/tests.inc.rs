// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[cfg(test)]
mod subject_tests {
    use super::*;
    use serde_json::json;

    const SUMS: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const MANIFEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const OTHER: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    fn subject(name: &str, digest: &str) -> ExpectedSubject {
        ExpectedSubject {
            name: name.to_owned(),
            digest: digest.to_owned(),
        }
    }

    fn statement(subjects: &[(&str, &str)]) -> serde_json::Value {
        json!({"subject": subjects
            .iter()
            .map(|(name, digest)| json!({"name": name, "digest": {"sha256": digest}}))
            .collect::<Vec<_>>()})
    }

    fn check(subjects: &[(&str, &str)], name: &str, digest: &str) -> bool {
        let (peer_name, peer_digest) = if name == "SHA256SUMS" {
            ("RUNNER_IMAGE_MANIFEST.json", MANIFEST)
        } else {
            ("SHA256SUMS", SUMS)
        };
        check_subjects(
            &statement(subjects),
            &subject(name, digest),
            &subject(peer_name, peer_digest),
        )
        .is_ok()
    }

    #[test]
    fn a_statement_naming_only_the_checksum_file_verifies_that_file() {
        assert!(check(&[("SHA256SUMS", SUMS)], "SHA256SUMS", SUMS));
    }

    #[test]
    fn a_statement_naming_only_the_manifest_verifies_the_manifest() {
        assert!(check(
            &[("RUNNER_IMAGE_MANIFEST.json", MANIFEST)],
            "RUNNER_IMAGE_MANIFEST.json",
            MANIFEST
        ));
    }

    #[test]
    fn a_statement_naming_both_files_verifies_either() {
        let both = [("SHA256SUMS", SUMS), ("RUNNER_IMAGE_MANIFEST.json", MANIFEST)];
        assert!(check(&both, "SHA256SUMS", SUMS));
        assert!(check(&both, "RUNNER_IMAGE_MANIFEST.json", MANIFEST));
    }

    #[test]
    fn a_statement_without_the_requested_subject_is_refused() {
        assert!(!check(
            &[("RUNNER_IMAGE_MANIFEST.json", MANIFEST)],
            "SHA256SUMS",
            SUMS
        ));
    }

    #[test]
    fn a_subject_with_another_digest_is_refused() {
        assert!(!check(&[("SHA256SUMS", OTHER)], "SHA256SUMS", SUMS));
    }

    #[test]
    fn a_peer_subject_with_another_digest_is_refused() {
        let skewed = [("SHA256SUMS", SUMS), ("RUNNER_IMAGE_MANIFEST.json", OTHER)];
        assert!(!check(&skewed, "SHA256SUMS", SUMS));
    }

    #[test]
    fn a_duplicated_subject_is_refused() {
        assert!(!check(
            &[("SHA256SUMS", SUMS), ("SHA256SUMS", SUMS)],
            "SHA256SUMS",
            SUMS
        ));
    }
}
