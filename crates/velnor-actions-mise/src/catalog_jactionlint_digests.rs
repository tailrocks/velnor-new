//! jactionlint release digests: what makes a `jactionlint` install verifiable.
//!
//! Mise installs `jactionlint` through its `github:jdx/jactionlint` backend,
//! which checks the release-API digest and the GitHub artifact attestation.
//! Both trust GitHub and the upstream release workflow. These rows are
//! Velnor's own control: the generated install step compares the installed
//! `jactionlint` executable with [`JactionlintDigest::executable_sha256`]
//! for the runner platform, modelled on the Rust channel-manifest check.
//!
//! Every row was measured when the version was bumped (2026-10-22), never at
//! generate time: the downloaded archive bytes, the release
//! `jactionlint_<version>_checksums.txt` and the release-API `digest` agree,
//! and the executable digest is the SHA-256 of `jactionlint` extracted from
//! that archive. Provenance evidence for the bump is recorded beside
//! [`super::JACTIONLINT_VERSION`]. Platforms use Mise's names, as in
//! `mise.lock`.

/// One measured `jactionlint` release asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JactionlintDigest {
    /// Exact jactionlint version.
    pub version: &'static str,
    /// Mise platform (`linux-x64`, `linux-arm64`, `macos-arm64`).
    pub platform: &'static str,
    /// Upstream release asset name.
    pub asset: &'static str,
    /// SHA-256 of the release archive.
    pub archive_sha256: &'static str,
    /// SHA-256 of the `jactionlint` executable inside the archive.
    pub executable_sha256: &'static str,
}

/// Measured digests, sorted by version then platform.
pub const JACTIONLINT_DIGESTS: &[JactionlintDigest] = &[
    JactionlintDigest {
        version: "2.3.0",
        platform: "linux-arm64",
        asset: "jactionlint_2.3.0_linux_arm64.tar.gz",
        archive_sha256: "deea7744e5769e1e5cc8c9898a838f80a941ec68476d9f2814a61474d5652dac",
        executable_sha256: "7b6c395e8fb7c407d7b3358da1deab23434a75840dc76768ddb30119ce1041c0",
    },
    JactionlintDigest {
        version: "2.3.0",
        platform: "linux-x64",
        asset: "jactionlint_2.3.0_linux_amd64.tar.gz",
        archive_sha256: "907e6f6a59e181879f7e6936e19ee2eca1cbc51de134fb85460cfa8d65a71f28",
        executable_sha256: "3566633b33780130c5bea5342ad8c5505092e40094c7d6e25ee74e382899a5c7",
    },
    JactionlintDigest {
        version: "2.3.0",
        platform: "macos-arm64",
        asset: "jactionlint_2.3.0_darwin_arm64.tar.gz",
        archive_sha256: "f96afa36578b21290203ae8df4fa7ac3bf5aba08ed288df28cb0530af7ca6e55",
        executable_sha256: "b6853a909613dc9381548b1d61d278e1c4b1ab8334212f67ea6efa46f1f5d34b",
    },
];

fn row(version: &str, platform: &str) -> Option<&'static JactionlintDigest> {
    JACTIONLINT_DIGESTS
        .iter()
        .find(|row| row.version == version && row.platform == platform)
}

/// Pinned archive SHA-256 for one exact version on one Mise platform.
#[must_use]
pub fn jactionlint_archive_sha256(version: &str, platform: &str) -> Option<&'static str> {
    row(version, platform).map(|row| row.archive_sha256)
}

/// Pinned executable SHA-256 for one exact version on one Mise platform.
#[must_use]
pub fn jactionlint_executable_sha256(version: &str, platform: &str) -> Option<&'static str> {
    row(version, platform).map(|row| row.executable_sha256)
}

#[cfg(test)]
mod tests {
    use super::{JACTIONLINT_DIGESTS, jactionlint_archive_sha256, jactionlint_executable_sha256};
    use crate::catalog::{JACTIONLINT_VERSION, validate_concrete_exact_version};

    const PLATFORMS: [&str; 3] = ["linux-arm64", "linux-x64", "macos-arm64"];

    fn is_sha256(value: &str) -> bool {
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }

    #[test]
    fn the_catalog_pin_has_every_runner_platform() {
        for platform in PLATFORMS {
            assert!(
                jactionlint_executable_sha256(JACTIONLINT_VERSION, platform).is_some(),
                "bump JACTIONLINT_VERSION together with its {platform} digests"
            );
            assert!(jactionlint_archive_sha256(JACTIONLINT_VERSION, platform).is_some());
        }
    }

    #[test]
    fn rows_are_exact_sorted_unique_and_lowercase_sha256() {
        let mut previous: Option<(Vec<u64>, &str)> = None;
        for row in JACTIONLINT_DIGESTS {
            validate_concrete_exact_version("jactionlint", row.version).expect("exact version");
            assert!(PLATFORMS.contains(&row.platform), "{}", row.platform);
            assert!(is_sha256(row.archive_sha256), "{}", row.asset);
            assert!(is_sha256(row.executable_sha256), "{}", row.asset);
            assert_ne!(row.archive_sha256, row.executable_sha256, "{}", row.asset);
            assert!(
                row.asset
                    .starts_with(&format!("jactionlint_{}_", row.version)),
                "{}",
                row.asset
            );
            let key: Vec<u64> = row
                .version
                .split('.')
                .map(|part| part.parse().expect("numeric"))
                .collect();
            if let Some(previous) = &previous {
                assert!(
                    (&previous.0, previous.1) < (&key, row.platform),
                    "{} {} sorted and unique",
                    row.version,
                    row.platform
                );
            }
            previous = Some((key, row.platform));
        }
        assert_eq!(
            JACTIONLINT_DIGESTS.last().map(|row| row.version),
            Some(JACTIONLINT_VERSION),
            "the newest rows are the catalog pin"
        );
    }

    #[test]
    fn unknown_versions_and_platforms_have_no_digest() {
        assert_eq!(jactionlint_executable_sha256("2.2.0", "linux-x64"), None);
        assert_eq!(
            jactionlint_executable_sha256(JACTIONLINT_VERSION, "windows-x64"),
            None
        );
        assert_eq!(jactionlint_archive_sha256("2.3", "linux-x64"), None);
    }
}
