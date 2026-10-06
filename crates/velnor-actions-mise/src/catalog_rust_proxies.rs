//! Repair the fixed Rustup proxy closure from its verified owned manager.
//!
//! Rustup 1.29.1 `src/cli/self_update.rs::install_proxies_with_opts`
//! creates links to the same manager executable. Preserve that behavior,
//! without invoking floating `rustup self update` or fetching tool payloads.

/// Rustup manager required by the isolated Rust backend installation.
pub const RUSTUP_VERSION: &str = "1.29.1";

/// SHA256 of the exact Linux x64 manager/installer, verified against upstream
/// `rustup/archive/1.29.1/x86_64-unknown-linux-gnu/rustup-init.sha256`.
pub const RUSTUP_SHA256_LINUX_AMD64: &str =
    "dda7234360b7f578ca8b0ddcb80145646fa61a67c1720a5abc7051b35c9fcb71";

/// Exact macOS arm64 manager/installer checksum; downloaded bytes verified
/// against the official `rustup/archive/1.29.1/aarch64-apple-darwin` archive.
pub const RUSTUP_SHA256_MACOS_ARM64: &str =
    "ec1b9233e7f72990ecd8e62063fa7f6c3dfc2bec8e97f88bff165f9100ac696a";

/// Fixed shell payload for the owned manager and required validation proxies.
///
/// Caller carries `CARGO_HOME`, `RUSTUP_HOME`, and the pinned toolchain.
/// A mismatched/missing manager fails preparation and must trigger the
/// owning tool-cache corruption fallback; no ambient manager is accepted.
#[must_use]
pub fn repair_script() -> String {
    repair_script_for_host(super::rust_bootstrap::RustHost::LinuxAmd64)
}

/// Fixed manager/proxy bytes repair without launching any proxy.
///
/// `SourceIntent` uses this data-only phase while its independent native
/// compiler authority owns all first-execution probes.
#[must_use]
pub fn repair_data_script() -> String {
    repair_data_script_for_host(super::rust_bootstrap::RustHost::LinuxAmd64)
}

/// Host-bound manager/proxy data repair with no version subprocesses.
#[must_use]
pub fn repair_data_script_for_host(host: super::rust_bootstrap::RustHost) -> String {
    format!(
        "set -eu; \
         test ! -L \"$CARGO_HOME\"; \
         test ! -L \"$CARGO_HOME/bin\"; \
         manager=\"$CARGO_HOME/bin/rustup\"; \
         test -f \"$manager\"; \
         test -x \"$manager\"; \
         test ! -L \"$manager\"; \
         printf '%s  %s\\n' '{sha}' \"$manager\" | {verify}; \
         for proxy in cargo rustc rustdoc cargo-clippy clippy-driver rustfmt cargo-fmt; do \
           destination=\"$CARGO_HOME/bin/$proxy\"; \
           if ! test \"$manager\" -ef \"$destination\" || test -L \"$destination\"; then \
             printf 'Repairing owned Rustup proxy: %s\\n' \"$proxy\"; \
             rm -f -- \"$destination\"; \
             test ! -e \"$destination\"; \
             test ! -L \"$destination\"; \
             ln \"$manager\" \"$destination\"; \
           fi; \
           test \"$manager\" -ef \"$destination\"; \
         done;",
        sha = host.sha256(),
        verify = host.sha256_command(),
    )
}

/// Host-bound manager repair using portable, owner-matched hardlink operations.
///
/// Preparation has one exclusive writer. Remove only the lexical proxy leaf,
/// verify it is absent, then link the verified manager without following a
/// destination symlink or invoking GNU-only `ln -T` on macOS.
#[must_use]
pub fn repair_script_for_host(host: super::rust_bootstrap::RustHost) -> String {
    format!(
        "{} \
         \"$CARGO_HOME/bin/rustc\" --version; \
         \"$CARGO_HOME/bin/cargo-clippy\" --version; \
         \"$CARGO_HOME/bin/rustfmt\" --version",
        repair_data_script_for_host(host),
    )
}

#[cfg(test)]
mod tests {
    use super::{RUSTUP_SHA256_LINUX_AMD64, repair_data_script, repair_script};

    #[test]
    fn repair_is_fixed_owned_digest_verified_and_download_free() {
        let script = repair_script();
        assert!(!script.contains('\n'));
        assert!(script.contains(RUSTUP_SHA256_LINUX_AMD64));
        assert!(script.contains("test ! -L \"$manager\""));
        assert!(script.contains("test \"$manager\" -ef \"$destination\""));
        assert!(script.contains("ln \"$manager\" \"$destination\""));
        assert!(!script.contains("self update"));
        assert!(!script.contains("curl"));
    }

    #[test]
    fn data_repair_has_no_proxy_or_manager_version_probe() {
        let script = repair_data_script();
        assert!(!script.contains('\n'));
        assert!(script.contains(RUSTUP_SHA256_LINUX_AMD64));
        assert!(script.contains("ln \"$manager\" \"$destination\""));
        assert!(!script.contains("--version"));
        assert!(!script.contains("rustup run"));
    }
}
