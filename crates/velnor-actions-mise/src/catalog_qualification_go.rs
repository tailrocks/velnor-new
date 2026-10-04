//! Closed official Go source-builder compiler authority.
//!
//! These records describe the exact official Go 1.27.1 toolchain archives
//! used by the native Actionlint source-build recipe. They are deliberately
//! separate from [`super::DistributionHost`]: Intel macOS is a compiler host
//! even though it is not a runtime distribution host.
//!
//! Evidence captured 2026-10-03 from the official Go download records and the
//! `go.googlesource.com/go` commit metadata. Archive and `go/bin/go` bytes were
//! hashed after safe extraction without execution. The full tree digest uses
//! `scripts/build_owned_actionlint.py::toolchain_tree`.

use crate::MiseError;
use sha2::{Digest, Sha256};

/// Exact compiler release admitted by the native source-build recipe.
pub(super) const GO_VERSION: &str = "go1.27.1";
/// Canonical Go source repository for the measured commit and tree.
pub(super) const SOURCE_REPOSITORY: &str = "https://go.googlesource.com/go";
/// Exact source commit for [`GO_VERSION`].
pub(super) const SOURCE_COMMIT: &str = "862c888e612ac346c7c4d99c9392bdfd265f33b0";
/// Exact source tree for [`SOURCE_COMMIT`].
pub(super) const SOURCE_TREE: &str = "cdb027640162ada52eac600797bff595a5767479";
/// Name of the full extracted-tree digest algorithm.
pub(super) const TRANSFORM_ABI: &str = "toolchain-tree-v1";
/// Exact regular-file count in each measured Go toolchain root.
pub(super) const TOOLCHAIN_FILE_COUNT: usize = 15_639;
/// Recipe scope: this compiler authority is never a runtime distribution.
pub(super) const RECIPE_PURPOSE: &str = "native-only-source-builder";

const ARCHIVE_ROOT: &str = "go";
const COMPILER_MEMBER: &str = "go/bin/go";

/// Native hosts with measured official Go compiler assets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceBuildCompilerHost {
    /// GNU Linux on AMD64.
    LinuxAmd64,
    /// Darwin on ARM64.
    MacosArm64,
    /// Darwin on AMD64.
    MacosAmd64,
}

impl SourceBuildCompilerHost {
    /// Exact source-builder target; unsupported targets never select a default.
    #[must_use]
    pub fn for_target(target: &str) -> Option<Self> {
        match target {
            "x86_64-unknown-linux-gnu" => Some(Self::LinuxAmd64),
            "aarch64-apple-darwin" => Some(Self::MacosArm64),
            "x86_64-apple-darwin" => Some(Self::MacosAmd64),
            _ => None,
        }
    }

    /// Exact Go target triple.
    #[must_use]
    pub const fn target_triple(self) -> &'static str {
        match self {
            Self::LinuxAmd64 => "x86_64-unknown-linux-gnu",
            Self::MacosArm64 => "aarch64-apple-darwin",
            Self::MacosAmd64 => "x86_64-apple-darwin",
        }
    }

    /// Exact Go operating-system value.
    #[must_use]
    pub const fn goos(self) -> &'static str {
        match self {
            Self::LinuxAmd64 => "linux",
            Self::MacosArm64 | Self::MacosAmd64 => "darwin",
        }
    }

    /// Exact Go architecture value.
    #[must_use]
    pub const fn goarch(self) -> &'static str {
        match self {
            Self::LinuxAmd64 | Self::MacosAmd64 => "amd64",
            Self::MacosArm64 => "arm64",
        }
    }
}

/// Exact official compiler and extracted-toolchain identity for one host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QualifiedBuildCompiler {
    host: SourceBuildCompilerHost,
    target: &'static str,
    goos: &'static str,
    goarch: &'static str,
    go_version: &'static str,
    asset_url: &'static str,
    archive_sha256: &'static str,
    primary_binary_sha256: &'static str,
    toolchain_tree_sha256: &'static str,
    source_repository: &'static str,
    source_commit: &'static str,
    source_tree: &'static str,
    archive_root: &'static str,
    compiler_member: &'static str,
    transform_abi: &'static str,
    toolchain_file_count: usize,
    native_only: bool,
    recipe_purpose: &'static str,
}

impl QualifiedBuildCompiler {
    /// Source-build host identity.
    #[must_use]
    pub const fn host(&self) -> SourceBuildCompilerHost {
        self.host
    }

    /// Exact target triple.
    #[must_use]
    pub const fn target_triple(&self) -> &'static str {
        self.target
    }

    /// Exact Go operating-system value.
    #[must_use]
    pub const fn goos(&self) -> &'static str {
        self.goos
    }

    /// Exact Go architecture value.
    #[must_use]
    pub const fn goarch(&self) -> &'static str {
        self.goarch
    }

    /// Exact Go release string.
    #[must_use]
    pub const fn go_version(&self) -> &'static str {
        self.go_version
    }

    /// Official Go archive URL.
    #[must_use]
    pub const fn asset_url(&self) -> &'static str {
        self.asset_url
    }

    /// SHA-256 of the downloaded archive.
    #[must_use]
    pub const fn archive_sha256(&self) -> &'static str {
        self.archive_sha256
    }

    /// Alias matching the publication receipt field name.
    #[must_use]
    pub const fn compiler_binary_sha256(&self) -> &'static str {
        self.primary_binary_sha256
    }

    /// SHA-256 of the complete extracted toolchain manifest.
    #[must_use]
    pub const fn toolchain_tree_sha256(&self) -> &'static str {
        self.toolchain_tree_sha256
    }

    /// Canonical source repository.
    #[must_use]
    pub const fn source_repository(&self) -> &'static str {
        self.source_repository
    }

    /// Exact compiler source commit.
    #[must_use]
    pub const fn source_commit(&self) -> &'static str {
        self.source_commit
    }

    /// Exact compiler source tree.
    #[must_use]
    pub const fn source_tree(&self) -> &'static str {
        self.source_tree
    }

    /// Archive root directory.
    #[must_use]
    pub const fn archive_root(&self) -> &'static str {
        self.archive_root
    }

    /// Exact compiler member inside the archive.
    #[must_use]
    pub const fn compiler_member(&self) -> &'static str {
        self.compiler_member
    }

    /// Full-tree manifest transform ABI.
    #[must_use]
    pub const fn transform_abi(&self) -> &'static str {
        self.transform_abi
    }

    /// Number of regular files in the measured toolchain root.
    #[must_use]
    pub const fn toolchain_file_count(&self) -> usize {
        self.toolchain_file_count
    }

    /// Whether the record is restricted to native source builds.
    #[must_use]
    pub const fn native_only(&self) -> bool {
        self.native_only
    }

    /// Recipe purpose bound by this authority.
    #[must_use]
    pub const fn recipe_purpose(&self) -> &'static str {
        self.recipe_purpose
    }

    /// SHA-256 identity over every immutable authority field.
    #[must_use]
    pub fn qualification_digest(&self) -> String {
        let fields = [
            "velnor-qualified-build-compiler-v1",
            self.host.target_triple(),
            self.target,
            self.goos,
            self.goarch,
            self.go_version,
            self.asset_url,
            self.archive_sha256,
            self.primary_binary_sha256,
            self.toolchain_tree_sha256,
            self.source_repository,
            self.source_commit,
            self.source_tree,
            self.archive_root,
            self.compiler_member,
            self.transform_abi,
            &self.toolchain_file_count.to_string(),
            if self.native_only {
                "native-only"
            } else {
                "runtime"
            },
            self.recipe_purpose,
        ];
        let mut digest = Sha256::new();
        for field in fields {
            digest.update(field.len().to_string().as_bytes());
            digest.update(b":");
            digest.update(field.as_bytes());
        }
        let digest = digest.finalize();
        encode_hex(&digest)
    }

    fn validate(&self) -> Result<(), MiseError> {
        if self.go_version != GO_VERSION
            || self.target != self.host.target_triple()
            || self.goos != self.host.goos()
            || self.goarch != self.host.goarch()
            || self.source_repository != SOURCE_REPOSITORY
            || self.source_commit != SOURCE_COMMIT
            || self.source_tree != SOURCE_TREE
            || self.archive_root != ARCHIVE_ROOT
            || self.compiler_member != COMPILER_MEMBER
            || self.transform_abi != TRANSFORM_ABI
            || self.toolchain_file_count != TOOLCHAIN_FILE_COUNT
            || !self.native_only
            || self.recipe_purpose != RECIPE_PURPOSE
        {
            return Err(contract("go compiler authority identity mismatch"));
        }
        if !self.asset_url.starts_with("https://go.dev/dl/")
            || !valid_hex(self.archive_sha256, 64)
            || !valid_hex(self.primary_binary_sha256, 64)
            || !valid_hex(self.toolchain_tree_sha256, 64)
            || !valid_hex(self.source_commit, 40)
            || !valid_hex(self.source_tree, 40)
        {
            return Err(contract("go compiler authority digest or URL invalid"));
        }
        let expected_url = match self.host {
            SourceBuildCompilerHost::LinuxAmd64 => "https://go.dev/dl/go1.27.1.linux-amd64.tar.gz",
            SourceBuildCompilerHost::MacosArm64 => "https://go.dev/dl/go1.27.1.darwin-arm64.tar.gz",
            SourceBuildCompilerHost::MacosAmd64 => "https://go.dev/dl/go1.27.1.darwin-amd64.tar.gz",
        };
        if self.asset_url != expected_url {
            return Err(contract("go compiler archive does not match host"));
        }
        Ok(())
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(*byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(*byte & 0x0f)]));
    }
    encoded
}

/// Build the measured official compiler record for one exact native host.
/// # Errors
/// Rejects unsupported versions or any invalid source/asset identity.
pub fn official_go(
    host: SourceBuildCompilerHost,
    exact_go_version: &str,
) -> Result<QualifiedBuildCompiler, MiseError> {
    if exact_go_version != GO_VERSION {
        return Err(MiseError::InvalidToolVersion {
            tool: "go".to_owned(),
            version: exact_go_version.to_owned(),
        });
    }
    let (asset_url, archive_sha256, primary_binary_sha256, toolchain_tree_sha256) = match host {
        SourceBuildCompilerHost::LinuxAmd64 => (
            "https://go.dev/dl/go1.27.1.linux-amd64.tar.gz",
            "63d339f0da5ab53635a56f2490a7984dfe12dfcff22ad749f63edaf590168445",
            "30969f97169d7f43fe6a085873d75613adc21e30818a8c61d95bd27275df4624",
            "93d419aad923f0c45760b3cc25a64aff07c80306076501e84c9d40c4ab682fd4",
        ),
        SourceBuildCompilerHost::MacosArm64 => (
            "https://go.dev/dl/go1.27.1.darwin-arm64.tar.gz",
            "ee215d57e0ec269c60cc9ceca68e6bda321ba9ee5afe24f4b0988703c2d87d12",
            "132b69336a1f809932a8a20b0201dbbb980e86e3a323ae32e893639d83d71598",
            "94afd0a97d5fe086fedad187a308a92cfba24842292700f725d7af20d9478216",
        ),
        SourceBuildCompilerHost::MacosAmd64 => (
            "https://go.dev/dl/go1.27.1.darwin-amd64.tar.gz",
            "8f8f52c6649542cf027bbc9b9c68d1ec042f9f34808a40413f0b8b3f66f3caa4",
            "285418143831d996755c236ca0938ad317b22edeeb1d61bfa082f50550399fe3",
            "a591cfd5be3a4eb2af7d6408c93dd6c76a34c1b1678ac00f240a99a176d08e1b",
        ),
    };
    let record = QualifiedBuildCompiler {
        host,
        target: host.target_triple(),
        goos: host.goos(),
        goarch: host.goarch(),
        go_version: GO_VERSION,
        asset_url,
        archive_sha256,
        primary_binary_sha256,
        toolchain_tree_sha256,
        source_repository: SOURCE_REPOSITORY,
        source_commit: SOURCE_COMMIT,
        source_tree: SOURCE_TREE,
        archive_root: ARCHIVE_ROOT,
        compiler_member: COMPILER_MEMBER,
        transform_abi: TRANSFORM_ABI,
        toolchain_file_count: TOOLCHAIN_FILE_COUNT,
        native_only: true,
        recipe_purpose: RECIPE_PURPOSE,
    };
    record.validate()?;
    Ok(record)
}

fn valid_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && !value.bytes().all(|byte| byte == b'0')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn contract(problem: &str) -> MiseError {
    MiseError::Contract {
        problem: problem.to_owned(),
    }
}

#[cfg(test)]
#[path = "catalog_qualification_go_tests.rs"]
mod tests;
