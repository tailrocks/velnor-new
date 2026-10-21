//! Official Gradle checksums, as published by `services.gradle.org`.
//!
//! Each row was read on 2026-10-20 (9.6.0 through 9.8.2: 2026-10-21) from
//! `https://services.gradle.org/distributions/gradle-<version>-{bin,all}.zip.sha256`
//! and `gradle-<version>-wrapper.jar.sha256` (the `wrapperChecksumUrl` of
//! `https://services.gradle.org/versions/all`). `velnor-actions-freshness`
//! is the intended maintainer; until that pipeline reads Gradle, rows are
//! added together with the evidence date, never derived from a
//! repository's own files and never typed by hand:
//! `scripts/refresh-gradle-catalog.sh <version>...` prints the rows from the
//! official `.sha256` files.

/// One released Gradle version and its three official digests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GradleRelease {
    /// Version, as in `distributionUrl`.
    pub version: &'static str,
    /// SHA-256 of `gradle-<version>-bin.zip`.
    pub bin_sha256: &'static str,
    /// SHA-256 of `gradle-<version>-all.zip`.
    pub all_sha256: &'static str,
    /// SHA-256 of the `gradle-wrapper.jar` that `gradle wrapper` writes.
    pub wrapper_jar_sha256: &'static str,
}

/// The catalog, ascending by version.
pub const RELEASES: [GradleRelease; 11] = [
    GradleRelease {
        version: "9.3.1",
        bin_sha256: "b266d5ff6b90eada6dc3b20cb090e3731302e553a27c5d3e4df1f0d76beaff06",
        all_sha256: "17f277867f6914d61b1aa02efab1ba7bb439ad652ca485cd8ca6842fccec6e43",
        wrapper_jar_sha256: "b3a875ddc1f044746e1b1a55f645584505f4a10438c1afea9f15e92a7c42ec13",
    },
    GradleRelease {
        version: "9.4.1",
        bin_sha256: "2ab2958f2a1e51120c326cad6f385153bb11ee93b3c216c5fccebfdfbb7ec6cb",
        all_sha256: "708d2c6ecc97ca9a11838ef64a6c2301151b8dd10387e22dc1a12c30557cab5b",
        wrapper_jar_sha256: "55243ef57851f12b070ad14f7f5bb8302daceeebc5bce5ece5fa6edb23e1145c",
    },
    GradleRelease {
        version: "9.5.0",
        bin_sha256: "553c78f50dafcd54d65b9a444649057857469edf836431389695608536d6b746",
        all_sha256: "a3c4ba4aca8f0075688b9c5b18939fd28e8cb4357c227da5c1d9f38343791439",
        wrapper_jar_sha256: "497c8c2a7e5031f6aa847f88104aa80a93532ec32ee17bdb8d1d2f67a194a9c7",
    },
    GradleRelease {
        version: "9.5.1",
        bin_sha256: "bafc141b619ad6350fd975fc903156dd5c151998cc8b058e8c1044ab5f7b031f",
        all_sha256: "c72fb9991f6025cbe337d52ba77e531b3faf62bdd3e348fe1ccee9f51c71adb0",
        wrapper_jar_sha256: "497c8c2a7e5031f6aa847f88104aa80a93532ec32ee17bdb8d1d2f67a194a9c7",
    },
    GradleRelease {
        version: "9.6.0",
        bin_sha256: "bbaeb2fef8710818cf0e261201dab964c572f92b942812df0c3620d62a529a01",
        all_sha256: "87a2216cc1f9122192d4e0fe905ffdf1b4c72cff797e9f733b174e157cadd396",
        wrapper_jar_sha256: "497c8c2a7e5031f6aa847f88104aa80a93532ec32ee17bdb8d1d2f67a194a9c7",
    },
    GradleRelease {
        version: "9.6.1",
        bin_sha256: "9c0f7faeeb306cb14e4279a3e084ca6b596894089a0638e68a07c945a32c9e14",
        all_sha256: "61ba77b3ff7167e60962763eb4bae79db7120c189b9544358d0ade3c1e712a83",
        wrapper_jar_sha256: "497c8c2a7e5031f6aa847f88104aa80a93532ec32ee17bdb8d1d2f67a194a9c7",
    },
    GradleRelease {
        version: "9.7.0",
        bin_sha256: "84fbba45c7f4c64abc77460e1c00f541e9f960e3c7ed2538f1ede19eacd873ae",
        all_sha256: "a9ecb5ac5c2ca40691e6527724d11d0b43b8c0a52825b77c09899f2a72d2d2bf",
        wrapper_jar_sha256: "7a9ce74cff467ca1bf60a4fcd9f05185acceda4d0f382434d393e17864262c5d",
    },
    GradleRelease {
        version: "9.7.1",
        bin_sha256: "acd53f1edaf02f1a8ff99879f8a34b302661a057d9b063ae9e35b552f804d20a",
        all_sha256: "92c1a136d76b5017732a66d2e0a648ebff00dd3687d8bff0d0047a1bd904fdf2",
        wrapper_jar_sha256: "7a9ce74cff467ca1bf60a4fcd9f05185acceda4d0f382434d393e17864262c5d",
    },
    GradleRelease {
        version: "9.8.0",
        bin_sha256: "bafd5ce9cfaea0fbccfdc8439a1ac42fbd4cd9c89dc9a988228d8a2639a58e6c",
        all_sha256: "46ac66d47f30f3dacfdf306e0b714a91a34fb94a22ba0a744b280933f47bc0cf",
        wrapper_jar_sha256: "238e777fcddd7e34f9708186085def2abd6e08e658505b38718d79d74c21abd5",
    },
    GradleRelease {
        version: "9.8.1",
        bin_sha256: "dce76f55f8e251a3a1f130eb120f30b3d271de2b76c9b0729d316b5a1b6dc01f",
        all_sha256: "5ceda8d2425eb2af196dec31b6fc1f38af2fb638fd621f86fa685805f2262224",
        wrapper_jar_sha256: "3b8a25775a69158b5ad2b1d17a80a88dc7b40a352a73eb27d06c612a7ce68e98",
    },
    GradleRelease {
        version: "9.8.2",
        bin_sha256: "21bd5da2de49b642904077f38c0efdc7778e92b3ed0889392dca61b97e389607",
        all_sha256: "a1bb7e9651ef09e4cd9a62d471564e0ee23b5757e5f21fd7358d7cfc21018fdf",
        wrapper_jar_sha256: "3b8a25775a69158b5ad2b1d17a80a88dc7b40a352a73eb27d06c612a7ce68e98",
    },
];

/// Catalog row for a version.
#[must_use]
pub fn release(version: &str) -> Option<&'static GradleRelease> {
    RELEASES.iter().find(|row| row.version == version)
}

/// True when `sha256` is the official wrapper-jar digest of any cataloged release.
#[must_use]
pub fn is_official_wrapper_jar(sha256: &str) -> bool {
    RELEASES.iter().any(|row| row.wrapper_jar_sha256 == sha256)
}
