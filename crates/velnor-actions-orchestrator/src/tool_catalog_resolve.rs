//! Resolve effective [`ToolCatalog`] from repository toolchain files and `.velnor/config.toml`.
//!
//! Enforces fail-closed agreement across `rust-toolchain.toml`, legacy
//! `rust-toolchain`, `[stacks.rust] toolchain`, and `[tools]`, omitting
//! `RUSTUP_TOOLCHAIN` from step environments when a repository toolchain
//! file is present so rustup honors the repository file directly.

use std::path::Path;

use velnor_actions_contract::VelnorConfig;
use velnor_actions_mise::ToolCatalog;
use velnor_actions_mise::catalog::{
    ACTIONLINT_VERSION, GH_VERSION, MR_BOXINGTON_VERSION, NEXTEST_VERSION, OPENTOFU_VERSION,
    RUST_VERSION, SHELLCHECK_VERSION, ZIZMOR_VERSION,
};
use velnor_actions_rust::{
    RUST_TOOLCHAIN_FILE, RUST_TOOLCHAIN_LEGACY_FILE, extract_rust_toolchain_channel,
    inspect_toolchain_file,
};

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};

/// Resolve the effective [`ToolCatalog`] for a repository root and parsed [`VelnorConfig`].
///
/// # Errors
///
/// Returns [`OrchestratorError::Config`] when a repository toolchain file has
/// an invalid or non-`X.Y.Z` channel, when `rust-toolchain.toml` and
/// `rust-toolchain` disagree, or when a repository toolchain file conflicts
/// with `[stacks.rust] toolchain` or `[tools] rust`.
pub(crate) fn resolve_tool_catalog(
    root: &Path,
    config: &VelnorConfig,
) -> Result<ToolCatalog, OrchestratorError> {
    let toml_tc = read_toolchain_file(root, RUST_TOOLCHAIN_FILE)?;
    let legacy_tc = read_toolchain_file(root, RUST_TOOLCHAIN_LEGACY_FILE)?;
    if let (Some((_, raw_toml, _)), Some((_, raw_legacy, _))) = (&toml_tc, &legacy_tc)
        && raw_toml != raw_legacy
    {
        return Err(OrchestratorError::config(
            RUST_TOOLCHAIN_FILE,
            "toolchain.channel",
            format!(
                "conflicting_rust_toolchain_files:{RUST_TOOLCHAIN_FILE}={raw_toml},{RUST_TOOLCHAIN_LEGACY_FILE}={raw_legacy}"
            ),
        ));
    }
    let file_tc = toml_tc.or(legacy_tc);
    let config_tc = config
        .stacks
        .rust
        .as_ref()
        .and_then(|rust| rust.toolchain.as_deref())
        .map(|tc| ("stacks.rust.toolchain", tc))
        .or_else(|| config.tools.get("rust").map(|tc| ("tools.rust", tc)));
    if let (Some((file_path, _, file_version)), Some((cfg_field, cfg_channel))) =
        (&file_tc, config_tc)
        && file_version != cfg_channel
    {
        return Err(OrchestratorError::config(
            CONFIG_REL,
            cfg_field,
            format!(
                "conflicting_rust_toolchain:{file_path}={file_version},{cfg_field}={cfg_channel}"
            ),
        ));
    }
    let rust_version = file_tc
        .as_ref()
        .map(|(_, _, version)| version.as_str())
        .or_else(|| config_tc.map(|(_, channel)| channel))
        .unwrap_or(RUST_VERSION);
    let export_rustup_toolchain = file_tc.is_none();
    let mr_boxington = config
        .tools
        .get("mr-boxington")
        .unwrap_or(MR_BOXINGTON_VERSION);
    let gh = config.tools.get("gh").unwrap_or(GH_VERSION);
    let actionlint = config.tools.get("actionlint").unwrap_or(ACTIONLINT_VERSION);
    let shellcheck = config.tools.get("shellcheck").unwrap_or(SHELLCHECK_VERSION);
    let zizmor = config.tools.get("zizmor").unwrap_or(ZIZMOR_VERSION);
    let nextest = config.tools.get("cargo-nextest").unwrap_or(NEXTEST_VERSION);
    let opentofu = config.tools.get("opentofu").unwrap_or(OPENTOFU_VERSION);
    let catalog = ToolCatalog::new(
        rust_version,
        mr_boxington,
        gh,
        actionlint,
        shellcheck,
        zizmor,
        nextest,
        opentofu,
    )
    .map_err(|err| OrchestratorError::Contract {
        problem: err.to_string(),
    })?;
    Ok(catalog.with_export_rustup_toolchain(export_rustup_toolchain))
}

fn read_toolchain_file(
    root: &Path,
    rel: &'static str,
) -> Result<Option<(&'static str, String, String)>, OrchestratorError> {
    let RepoRead::Text(text) = read_repo_file(root, rel, MAX_REPO_FILE_BYTES)? else {
        return Ok(None);
    };
    let version = extract_rust_toolchain_channel(rel, &text).map_err(|problem| {
        OrchestratorError::config(
            rel,
            "toolchain.channel",
            format!("invalid_rust_toolchain:{problem}"),
        )
    })?;
    let raw_channel = inspect_toolchain_file(rel, Some(&text))
        .ok()
        .and_then(|insp| insp.spec)
        .and_then(|spec| spec.channel)
        .unwrap_or_else(|| version.clone());
    Ok(Some((rel, raw_channel, version)))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use velnor_actions_contract::{RustStackConfig, VelnorConfig};
    use velnor_actions_mise::PinnedTool;

    use super::resolve_tool_catalog;
    use crate::pins::tests::config_with;

    #[test]
    fn resolves_repo_toolchain_file_and_omits_rustup_toolchain_export() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.94.1\"\n",
        )
        .expect("write toolchain");
        let cfg = config_with(BTreeMap::new());
        let catalog = resolve_tool_catalog(dir.path(), &cfg).expect("resolve");
        assert_eq!(catalog.version(PinnedTool::Rust), "1.94.1");
        assert!(!catalog.exports_rustup_toolchain());
        assert_eq!(catalog.rustup_toolchain_env(), None);
    }

    #[test]
    fn rejects_conflict_between_repo_toolchain_file_and_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.94.1\"\n",
        )
        .expect("write toolchain");
        let mut cfg = config_with(BTreeMap::new());
        let mut rust = RustStackConfig::default_config();
        rust.toolchain = Some("1.98.1".to_owned());
        cfg.stacks.rust = Some(rust);
        let err = resolve_tool_catalog(dir.path(), &cfg).expect_err("conflict must fail");
        assert!(
            err.to_string().contains(
                "conflicting_rust_toolchain:rust-toolchain.toml=1.94.1,stacks.rust.toolchain=1.98.1"
            ),
            "got {err}"
        );
    }

    #[test]
    fn resolves_config_only_rust_and_tool_overrides_with_rustup_export() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut cfg: VelnorConfig = config_with(BTreeMap::new());
        cfg.tools.overrides = BTreeMap::from([
            ("rust".to_owned(), "1.94.1".to_owned()),
            ("cargo-nextest".to_owned(), "0.9.150".to_owned()),
            ("actionlint".to_owned(), "1.7.13".to_owned()),
        ]);
        let catalog = resolve_tool_catalog(dir.path(), &cfg).expect("resolve");
        assert_eq!(catalog.version(PinnedTool::Rust), "1.94.1");
        assert_eq!(catalog.version(PinnedTool::Nextest), "0.9.150");
        assert_eq!(catalog.version(PinnedTool::Actionlint), "1.7.13");
        assert!(catalog.exports_rustup_toolchain());
        assert_eq!(catalog.rustup_toolchain_env(), Some("1.94.1"));
    }

    #[test]
    fn resolves_target_qualified_exact_toolchain_and_rejects_non_exact_channels() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cfg = config_with(BTreeMap::new());
        std::fs::write(
            dir.path().join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.99.1-x86_64-unknown-linux-gnu\"\n",
        )
        .expect("write target-qualified toolchain");
        let catalog = resolve_tool_catalog(dir.path(), &cfg).expect("resolve target-qualified");
        assert_eq!(catalog.version(PinnedTool::Rust), "1.99.1");
        assert!(!catalog.exports_rustup_toolchain());

        std::fs::write(
            dir.path().join("rust-toolchain"),
            "1.99.1-aarch64-apple-darwin\n",
        )
        .expect("write legacy toolchain with different target");
        let conflict_err =
            resolve_tool_catalog(dir.path(), &cfg).expect_err("target mismatch must fail");
        assert!(
            conflict_err.to_string().contains(
                "conflicting_rust_toolchain_files:rust-toolchain.toml=1.99.1-x86_64-unknown-linux-gnu,rust-toolchain=1.99.1-aarch64-apple-darwin"
            ),
            "got {conflict_err}"
        );
        std::fs::remove_file(dir.path().join("rust-toolchain")).expect("remove legacy toolchain");

        for bad in [
            "[toolchain]\nchannel = \"stable\"\n",
            "[toolchain]\nchannel = \"1.99\"\n",
            "[toolchain]\nchannel = \"nightly-2026-10-01\"\n",
            "[toolchain]\n",
            "[[[\n",
        ] {
            std::fs::write(dir.path().join("rust-toolchain.toml"), bad).expect("write bad");
            let err = resolve_tool_catalog(dir.path(), &cfg).expect_err("non-exact must fail");
            assert!(
                err.to_string().contains("invalid_rust_toolchain:"),
                "expected invalid_rust_toolchain for {bad:?}, got {err}"
            );
        }
    }
}
