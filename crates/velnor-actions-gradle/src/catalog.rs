//! Official Gradle checksums, as published by `services.gradle.org`.
//!
//! Each row was read on 2026-10-20 from
//! `https://services.gradle.org/distributions/gradle-<version>-{bin,all}.zip.sha256`
//! and `gradle-<version>-wrapper.jar.sha256` (the `wrapperChecksumUrl` of
//! `https://services.gradle.org/versions/all`). `velnor-actions-freshness`
//! is the intended maintainer; until that pipeline reads Gradle, rows are
//! added by hand together with the evidence date, never derived from a
//! repository's own files.

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
pub const RELEASES: [GradleRelease; 4] = [
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
