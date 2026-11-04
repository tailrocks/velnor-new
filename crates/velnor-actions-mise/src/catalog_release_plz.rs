// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualifiedReleasePlz {
    pub target: &'static str,
    pub archive_sha256: &'static str,
    pub executable_sha256: &'static str,
    pub checksum_command: &'static str,
}

#[must_use]
pub fn qualified_release_plz(version: &str, runner: &str) -> Option<QualifiedReleasePlz> {
    let (target, archive_sha256, executable_sha256, checksum_command) = match (version, runner) {
        ("0.4.3", "ubuntu-22.04" | "ubuntu-24.04" | "ubuntu-26.04") => (
            "x86_64-unknown-linux-musl",
            "4a4cfa92ea2abaee02b40daa596a95ca2725816a6a32cb03a8f48ddead287681",
            "678330a1f03330fb375d15be8d6e094d0f65d2b70e2af417c69fcec78fa74466",
            "/usr/bin/sha256sum",
        ),
        ("0.4.3", "macos-14" | "macos-15" | "macos-15-arm64" | "macos-26") => (
            "aarch64-apple-darwin",
            "b2c6e636199c7fa9dae8838fbebb49cb99933908eb7dd67f25ec831bb38e1c52",
            "d725885957dfe31407a88a5eed28b7e402b47bd0abdff3f5655c896d7a03d723",
            "/usr/bin/shasum -a 256",
        ),
        _ => return None,
    };
    Some(QualifiedReleasePlz {
        target,
        archive_sha256,
        executable_sha256,
        checksum_command,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualifiedCargoSemverChecks {
    pub version: &'static str,
    pub target: &'static str,
    pub archive_sha256: &'static str,
    pub executable_sha256: &'static str,
}

#[must_use]
pub fn qualified_cargo_semver_checks(runner: &str) -> Option<QualifiedCargoSemverChecks> {
    (runner == "ubuntu-24.04").then_some(QualifiedCargoSemverChecks {
        version: "0.51.0",
        target: "x86_64-unknown-linux-gnu",
        archive_sha256: "cacadafaa5cd27ae6d05a74a103266917ebab4c5e43c898360e177d4fbf06a65",
        executable_sha256: "bc9eb06ce04e97d6160c875ffdee1f6e56793e423dea025f344826be03e66829",
    })
}
