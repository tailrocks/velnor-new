//! Qualified exact baseline tool versions and artifact digests.

/// Qualified mise runner release (tag `v2026.10.0`).
/// Source: `https://api.github.com/repos/jdx/mise/releases/latest`; checked 2026-10-03.
pub const MISE_VERSION: &str = "2026.10.0";
/// Qualified Rust stable toolchain.
/// Source: `https://static.rust-lang.org/dist/channel-rust-stable.toml`; checked 2026-09-28.
pub const RUST_VERSION: &str = "1.98.1";
/// Qualified `mr-boxington` tool (binary on PATH is `mbx`; tag `v1.21.1`).
/// Source: `https://api.github.com/repos/jdx/mr-boxington/releases/latest`; checked 2026-10-03.
pub const MR_BOXINGTON_VERSION: &str = "1.21.1";
/// Qualified GitHub CLI (tag `v2.102.0`).
/// Source: `https://api.github.com/repos/cli/cli/releases/latest`; checked 2026-09-30.
pub const GH_VERSION: &str = "2.102.0";
/// Qualified actionlint release (tag `v1.7.12`).
/// Source: `https://api.github.com/repos/rhysd/actionlint/releases/latest`; checked 2026-09-28.
pub const ACTIONLINT_VERSION: &str = "1.7.12";
/// Qualified shellcheck release (tag `v0.11.0`).
/// Source: `https://api.github.com/repos/koalaman/shellcheck/releases/latest`; checked 2026-09-28.
pub const SHELLCHECK_VERSION: &str = "0.11.0";
/// Qualified zizmor tool release (`zizmorcore/zizmor` tag `v1.30.1`, stable, published 2026-09-09).
/// Source: `https://api.github.com/repos/zizmorcore/zizmor/releases/latest`; checked 2026-09-28.
pub const ZIZMOR_VERSION: &str = "1.30.1";
/// Qualified cargo-nextest release (`nextest-rs/nextest` tag
/// `cargo-nextest-0.9.146`, published 2026-09-21).
/// Source: `https://crates.io/api/v1/crates/cargo-nextest`; checked 2026-09-29.
pub const NEXTEST_VERSION: &str = "0.9.146";
/// Qualified `OpenTofu` engine release (tag `v1.13.1`).
/// Source: `https://github.com/opentofu/opentofu/releases/tag/v1.13.1`; checked 2026-10-02.
/// Install: bare `opentofu@<exact>` via the aqua backend (isolated probe
/// passed 2026-10-02); the mise version index lags the release, so pin
/// exact, never float.
pub const OPENTOFU_VERSION: &str = "1.13.1";
/// sha256 of `tofu_1.13.1_linux_amd64.tar.gz` (`v1.13.1` `SHA256SUMS`,
/// release-API `digest`, and fetched bytes agree; verified 2026-10-02).
/// This is the catalog digest: the runner fleet is x64-Linux (§4.3).
pub const OPENTOFU_SHA256_LINUX_AMD64: &str =
    "378ada19d4bc70c43732004e8159be771b23b9a5afdf059e5f8a2b3fa2c70a69";
/// sha256 of `tofu_1.13.1_linux_arm64.tar.gz` (same `SHA256SUMS`, verified 2026-10-02).
pub const OPENTOFU_SHA256_LINUX_ARM64: &str =
    "9c1ef375aa1852db0b2888aa921b640c71f8140d4682aa4fec99378a64fa7dc3";
/// sha256 of `tofu_1.13.1_darwin_amd64.tar.gz` (same `SHA256SUMS`, verified 2026-10-02).
pub const OPENTOFU_SHA256_DARWIN_AMD64: &str =
    "a73720443ba38712d7d96dc1e857add02c15a790919c653ad07492e9952f8c27";
/// sha256 of `tofu_1.13.1_darwin_arm64.tar.gz` (same `SHA256SUMS`, verified 2026-10-02).
pub const OPENTOFU_SHA256_DARWIN_ARM64: &str =
    "be78f659f04ef06a9dbd9b3934d46af95d787a3aa38396d459dea395261816a9";
/// Qualified release-plz coordinator release (tag `release-plz-v0.3.169`).
/// Source: `https://crates.io/api/v1/crates/release-plz`; checked 2026-09-30.
pub const RELEASE_PLZ_VERSION: &str = "0.3.169";
