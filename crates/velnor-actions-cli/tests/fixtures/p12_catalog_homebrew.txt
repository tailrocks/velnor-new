//! Exact Homebrew source and its Linux portable Ruby authority.
//!
//! Qualified from the official `Homebrew/brew` tag and immutable source files,
//! checked 2026-10-03. The preparation recipe must verify the checked-out commit
//! and force the vendor Ruby installer, which verifies these blob digests.
//! Homebrew is acquired from Git, not a fictional Mise tool selector.

/// Official Homebrew release tag.
pub const VERSION: &str = "7.0.7";
/// Commit resolved by the official `7.0.7` tag.
pub const SOURCE_SHA: &str = "8e858db5584704dcd469b8e826228c0d5a5a94f6";
/// Immutable official release source.
pub const SOURCE_URL: &str =
    "https://github.com/Homebrew/brew/tree/8e858db5584704dcd469b8e826228c0d5a5a94f6";
/// Ruby version in the pinned source's `Library/Homebrew/vendor/portable-ruby-version`.
pub const PORTABLE_RUBY_VERSION: &str = "4.0.7";
/// SHA-256 in the pinned source's `Library/Homebrew/vendor/portable-ruby-x86_64-linux`.
pub const PORTABLE_RUBY_X86_64_LINUX_SHA256: &str =
    "bf2a9bf102694d40084ed436b06a1566dded60a519f4d1879c90c81046e11081";
/// SHA-256 in the pinned source's `Library/Homebrew/vendor/portable-ruby-arm64-linux`.
pub const PORTABLE_RUBY_ARM64_LINUX_SHA256: &str =
    "c9b75dd6bd9578921f3ce739dacae8866698c3399c0f83574a5d5fadda2aab2d";
