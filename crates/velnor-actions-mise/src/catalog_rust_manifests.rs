//! Rust channel-manifest digests: what makes a `core:rust` install verifiable.
//!
//! Mise's `core:rust` backend installs through rustup and records no
//! checksum in `mise.lock`. rustup verifies every component it downloads
//! against the SHA-256 in the channel manifest it fetched, so pinning the
//! manifest's own SHA-256 here, per Rust version, closes the chain: the
//! generated `Verify Rust channel manifest` step fetches
//! `https://static.rust-lang.org/dist/channel-rust-<version>.toml`, checks it
//! against [`RUST_CHANNEL_MANIFESTS`], and requires every hash in the
//! installed toolchain's `lib/rustlib/multirust-channel-manifest.toml` to
//! appear in that verified manifest.
//!
//! Every row was read from static.rust-lang.org on 2026-10-22: the manifest
//! bytes were hashed locally and compared with the published
//! `channel-rust-<version>.toml.sha256`. Rows are sorted by version and
//! cover every stable release from 1.85.0 (edition 2024) to the catalog pin.

/// `(exact Rust version, channel-rust-<version>.toml SHA-256, manifest date)`.
pub const RUST_CHANNEL_MANIFESTS: &[(&str, &str, &str)] = &[
    (
        "1.85.0",
        "009e8b5ff43f12bf644b5e5b9fd89f96453072062a450c623882f64e85405da5",
        "2025-02-20",
    ),
    (
        "1.85.1",
        "1e7dae690cd12e27405a97e64704917489a27c650201bf47780a936055d67909",
        "2025-03-18",
    ),
    (
        "1.86.0",
        "5ffe190473b7896d1f39e9d0ddfa04bec72000f25897669bb296814e10ceba42",
        "2025-04-03",
    ),
    (
        "1.87.0",
        "2949b5ea91e3f9c45e75ff2fc6cfc7776616c693ab599ed43abc2120b7522415",
        "2025-05-15",
    ),
    (
        "1.88.0",
        "431b7c5c0b9a511d8e31d29b378bbc74124e8521f14beb92d3a5a5f7e7e55449",
        "2025-06-26",
    ),
    (
        "1.89.0",
        "fbd1662e100e7b305908ece23b441cb7534eadfa6336c5f173ff08d1cec174a1",
        "2025-08-07",
    ),
    (
        "1.90.0",
        "489c19f20d331765ab2835661eb546de90f6446a107a8db83045e7371e45cae2",
        "2025-09-18",
    ),
    (
        "1.91.0",
        "d9e59cdf15532aa839c0a4871b0b755e833f914042eb2dcc59f2a0ef8667f9f6",
        "2025-10-30",
    ),
    (
        "1.91.1",
        "483bb8b271168ee654e3be4f111beefa23b9d0c8b7f4a563a8278978dbe982e5",
        "2025-11-10",
    ),
    (
        "1.92.0",
        "b2a49624353173ecdacf59c158c00929300606e1963f6e4609fb483a508402d0",
        "2025-12-11",
    ),
    (
        "1.93.0",
        "beb6ba4e41c84e9c11c80e6804a007497d0c8ba0810cd403fabc8f4a9c45b1f8",
        "2026-01-22",
    ),
    (
        "1.93.1",
        "4812a3c610bacc74eed12c89c312e540722dccccd867bd55096402da13b3a516",
        "2026-02-12",
    ),
    (
        "1.94.0",
        "aaa177def36e01d539bee6bde95295230b9ce378f81057845db8d0ebe97898ee",
        "2026-03-05",
    ),
    (
        "1.94.1",
        "cc2f04dfc883549d683c8cc2a9393f523a3dfbd931f5d5eaef00303cca64a60d",
        "2026-03-26",
    ),
    (
        "1.95.0",
        "821ff14e4c4a1cbe1e8915f35aff0a3fbbdf8d293ad48ab8f31e3b0440c581f9",
        "2026-04-16",
    ),
    (
        "1.96.0",
        "9af50610e1d82699f78a40b985c9277ae1a2c5a0bec86ae430cfe58832038285",
        "2026-05-28",
    ),
    (
        "1.96.1",
        "87eb76c53073e72b766083bed5530820694253b832a762d8385bda5759f03975",
        "2026-06-30",
    ),
    (
        "1.97.0",
        "3804d2666f7c12ce64205baa69b6be52f910b45b158091013264beb7aa1de7f5",
        "2026-07-09",
    ),
    (
        "1.97.1",
        "03569b1886ceb5c05276b50c8431ab111de944cd6140fe1fa7d821dd8e0f29cf",
        "2026-07-16",
    ),
    (
        "1.98.0",
        "3f7d139b73bbbd0004ef6e58b430831c68cdad2b1f64ee2eb35d54c09199489a",
        "2026-08-20",
    ),
    (
        "1.98.1",
        "a7c8774a5fd8441c997d94c029776cbc5eb111e9d72ab5d256fa69866644347e",
        "2026-09-03",
    ),
    (
        "1.99.0",
        "ce6dddc886364f8d786514771212cebe9b731ba82d6b859951c6b0ccc516b6a2",
        "2026-10-01",
    ),
    (
        "1.99.1",
        "fb152be80975c194aa5fc684b2aec3a6e14e2b855efe2faa4e5f8ae6e590f463",
        "2026-10-15",
    ),
];

/// Pinned channel-manifest SHA-256 for one exact Rust version.
#[must_use]
pub fn rust_channel_manifest_sha256(version: &str) -> Option<&'static str> {
    RUST_CHANNEL_MANIFESTS
        .iter()
        .find(|(known, _, _)| *known == version)
        .map(|(_, digest, _)| *digest)
}

#[cfg(test)]
mod tests {
    use super::{RUST_CHANNEL_MANIFESTS, rust_channel_manifest_sha256};
    use crate::catalog::{RUST_VERSION, validate_exact_version};

    #[test]
    fn the_catalog_rust_pin_has_a_manifest_digest() {
        assert!(
            rust_channel_manifest_sha256(RUST_VERSION).is_some(),
            "bump RUST_VERSION together with its channel-manifest digest"
        );
    }

    #[test]
    fn rows_are_exact_unique_sorted_and_lowercase_sha256() {
        let mut previous: Option<Vec<u64>> = None;
        for (version, digest, date) in RUST_CHANNEL_MANIFESTS {
            validate_exact_version("rust", version).expect("exact version");
            assert_eq!(digest.len(), 64, "{version}");
            assert!(
                digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "{version}"
            );
            assert_eq!(date.len(), 10, "{version}");
            let key: Vec<u64> = version
                .split('.')
                .map(|part| part.parse().expect("numeric"))
                .collect();
            if let Some(previous) = &previous {
                assert!(previous < &key, "{version} sorted and unique");
            }
            previous = Some(key);
        }
        assert_eq!(
            RUST_CHANNEL_MANIFESTS.last().map(|row| row.0),
            Some(RUST_VERSION),
            "the newest row is the catalog pin"
        );
    }

    #[test]
    fn unknown_versions_have_no_digest() {
        assert_eq!(rust_channel_manifest_sha256("1.84.1"), None);
        assert_eq!(rust_channel_manifest_sha256("1.99"), None);
    }
}
