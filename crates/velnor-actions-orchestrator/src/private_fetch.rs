//! `private-fetch-v1` and `private-revoke-v1`: the Velnor-owned steps between
//! token mint and every repository-controlled step (HD-7).
//!
//! The fetch recomputes the token allowlist from the checked-out data and
//! refuses to run when it differs from the one rendered into the mint step,
//! fetches Cargo git dependencies (`cargo fetch --locked`, no build scripts),
//! release assets, Maven bundles and git trees, verifies every pin (tag commit
//! and SHA-256), and extracts through the hardened archive reader. The revoke
//! deletes the installation token and requires the API to then reject it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use velnor_actions_contract::config::{PrivateSourcesConfig, split_repository};
use velnor_actions_contract::deps_lock::{DepsLock, LockedAsset};
use velnor_actions_workflow_renderer::private_source_steps::{
    PRIVATE_OWNER_ENV, PRIVATE_REPOSITORIES_ENV, PRIVATE_ROOTS_ENV, PRIVATE_SOURCES_ENV,
};

use crate::OrchestratorError;
use crate::check_runtime::preparation::acquisition::archive::ArchiveBudget;
use crate::config::deps_layout::maven_bundle_dir;
use crate::config::deps_lock::{FoundAsset, parse_commit, parse_release_asset, read_deps_lock};
use crate::config::load_config;
use crate::config::private_fetch_root::{extract_into, prepare_deps_root};
use crate::config::private_fetch_session::{Session, Token, scrub_dir};
use crate::config::private_fetch_verified::{Verified, VerifiedSource};
use crate::config::private_sources::{SourceKind, job_allowlist};
use crate::discover::workspace_manifest;
use crate::internal::internal;

#[path = "private_fetch_owned_env.rs"]
mod owned_env;
use owned_env::owned_fetch_env;

/// Operation names (shared with the contract and the renderer).
pub const PRIVATE_FETCH_OP: &str = velnor_actions_contract::PRIVATE_FETCH_OPERATION;
/// Revoke operation name.
pub const PRIVATE_REVOKE_OP: &str = velnor_actions_contract::PRIVATE_REVOKE_OPERATION;

const UPPER_HEX: &[u8; 16] = b"0123456789ABCDEF";
const JSON: &str = "application/vnd.github+json";
const OCTET: &str = "application/octet-stream";
const JSON_LIMIT: u64 = 4 * 1024 * 1024;

/// Parsed and validated fetch input.
#[derive(Debug)]
pub(crate) struct FetchRequest {
    pub(crate) owner: String,
    pub(crate) repositories: Vec<String>,
    pub(crate) kinds: BTreeSet<SourceKind>,
    pub(crate) roots: Vec<String>,
    pub(crate) workspace: PathBuf,
    pub(crate) runner_temp: PathBuf,
    pub(crate) managed_cargo_home: PathBuf,
}

/// What one fetch retrieved.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct FetchSummary {
    pub(crate) cargo_roots: usize,
    pub(crate) assets: usize,
    pub(crate) trees: usize,
}

/// Transport the fetch uses; the real one is the credential [`Session`].
pub(crate) trait FetchBackend {
    /// The resolved Rust toolchain uses the `rust@<version>` form.
    fn cargo_fetch(&self, manifest: &Path, rust: &str) -> Result<(), OrchestratorError>;
    fn download(&self, route: &str, accept: &str, dest: &Path) -> Result<(), OrchestratorError>;
}

impl FetchBackend for Session {
    fn cargo_fetch(&self, manifest: &Path, rust: &str) -> Result<(), OrchestratorError> {
        Session::cargo_fetch(self, manifest, rust)
    }

    fn download(&self, route: &str, accept: &str, dest: &Path) -> Result<(), OrchestratorError> {
        Session::download(self, route, accept, dest)
    }
}

impl FetchRequest {
    /// Parse the step env. The token is validated by the caller.
    ///
    /// # Errors
    /// Rejects missing or malformed values.
    pub(crate) fn parse(get: &dyn Fn(&str) -> Option<String>) -> Result<Self, OrchestratorError> {
        let need = |key: &str| {
            get(key)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| internal(&format!("private_fetch_missing_env:{key}")))
        };
        let owner = need(PRIVATE_OWNER_ENV)?;
        if !velnor_actions_contract::config::is_valid_github_owner(&owner) {
            return Err(internal("private_fetch_owner_invalid"));
        }
        let repositories: Vec<String> = need(PRIVATE_REPOSITORIES_ENV)?
            .split(',')
            .map(str::to_owned)
            .collect();
        if !repositories.windows(2).all(|pair| pair[0] < pair[1])
            || !repositories
                .iter()
                .all(|name| velnor_actions_contract::config::is_valid_github_repo_name(name))
        {
            return Err(internal("private_fetch_repositories_invalid"));
        }
        let kinds = need(PRIVATE_SOURCES_ENV)?
            .split(',')
            .map(SourceKind::parse)
            .collect::<Option<BTreeSet<_>>>()
            .ok_or_else(|| internal("private_fetch_sources_invalid"))?;
        let roots = get(PRIVATE_ROOTS_ENV)
            .unwrap_or_default()
            .split(',')
            .filter(|root| !root.is_empty())
            .map(|root| {
                let root = if root == "." { "" } else { root };
                velnor_actions_contract::validate_fetch_root(root)
                    .map(|()| root.to_owned())
                    .map_err(|_| internal("private_fetch_root_invalid"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let absolute = |key: &str| -> Result<PathBuf, OrchestratorError> {
            let path = PathBuf::from(need(key)?);
            path.is_absolute()
                .then_some(path)
                .ok_or_else(|| internal(&format!("private_fetch_path_relative:{key}")))
        };
        Ok(Self {
            owner,
            repositories,
            kinds,
            roots,
            workspace: absolute("GITHUB_WORKSPACE")?,
            runner_temp: absolute("RUNNER_TEMP")?,
            managed_cargo_home: absolute("MISE_CARGO_HOME")?,
        })
    }
}

/// Verify the request against the repository data and perform the fetch.
///
/// # Errors
/// Fails closed on allowlist drift, a moved tag, a digest mismatch, an
/// unsafe archive, or any transport failure.
pub(crate) fn execute(
    request: &FetchRequest,
    backend: &dyn FetchBackend,
) -> Result<FetchSummary, OrchestratorError> {
    let config = load_config(&request.workspace)?;
    let private = config
        .private_sources
        .as_ref()
        .ok_or_else(|| internal("private_fetch_without_private_sources"))?;
    if private.owner != request.owner {
        return Err(internal("private_fetch_owner_mismatch"));
    }
    let lock = read_deps_lock(&request.workspace)?;
    check_lock(private, lock.as_ref())?;
    let expected = job_allowlist(
        &request.workspace,
        private,
        lock.as_ref(),
        &request.roots,
        &request.kinds,
    )?;
    if expected != request.repositories {
        return Err(internal("private_allowlist_drift"));
    }
    let deps = prepare_deps_root(&request.workspace)?;
    let downloads = request.runner_temp.join("velnor").join("private-downloads");
    scrub_dir(&downloads)?;
    std::fs::create_dir_all(&downloads)
        .map_err(|err| OrchestratorError::io(downloads.display().to_string(), err.to_string()))?;
    let result = fetch_all(request, &config, backend, lock.as_ref(), &deps, &downloads);
    drop(scrub_dir(&downloads));
    result
}

fn check_lock(
    config: &PrivateSourcesConfig,
    lock: Option<&DepsLock>,
) -> Result<(), OrchestratorError> {
    match lock {
        Some(lock) => lock
            .check_matches(config, velnor_actions_contract::config::DEPS_LOCK_PATH)
            .map_err(crate::config::config_error),
        None if config.source_ids().next().is_none() => Ok(()),
        None => Err(internal("deps_lock_missing")),
    }
}

fn fetch_all(
    request: &FetchRequest,
    config: &velnor_actions_contract::VelnorConfig,
    backend: &dyn FetchBackend,
    lock: Option<&DepsLock>,
    deps: &Path,
    downloads: &Path,
) -> Result<FetchSummary, OrchestratorError> {
    let mut summary = FetchSummary::default();
    if request.kinds.contains(&SourceKind::Cargo) && !request.roots.is_empty() {
        // Resolve Rust as generation does, including the default pin when no project version is declared.
        // The generated resolver exports this version as `VELNOR_TOOL_RUST` for installation in the managed Cargo home.
        let rust = crate::tool_catalog_resolve::resolve_tool_catalog(&request.workspace, config)?
            .tool_spec(velnor_actions_mise::PinnedTool::Rust);
        for root in &request.roots {
            backend.cargo_fetch(&request.workspace.join(workspace_manifest(root)), &rust)?;
            summary.cargo_roots += 1;
        }
    }
    let Some(lock) = lock else {
        return Ok(summary);
    };
    let mut budget = ArchiveBudget::new();
    let mut verified = Vec::new();
    let mut assets: Vec<(SourceKind, &LockedAsset, PathBuf, String)> = Vec::new();
    if request.kinds.contains(&SourceKind::MavenBundles) {
        assets.extend(lock.maven_bundles.iter().map(|bundle| {
            (
                SourceKind::MavenBundles,
                bundle,
                maven_bundle_dir(deps, &bundle.id),
                format!("maven/{}", bundle.id),
            )
        }));
    }
    if request.kinds.contains(&SourceKind::ReleaseAssets) {
        for asset in &lock.release_assets {
            let target = asset
                .extract_to
                .as_deref()
                .ok_or_else(|| internal("private_asset_without_extract_to"))?;
            assets.push((
                SourceKind::ReleaseAssets,
                asset,
                deps.join(target),
                target.to_owned(),
            ));
        }
    }
    for (kind, asset, destination, relative) in assets {
        fetch_asset(backend, asset, &destination, downloads, &mut budget)?;
        verified.push(VerifiedSource::asset(kind, asset, &relative));
        summary.assets += 1;
    }
    if request.kinds.contains(&SourceKind::GitTrees) {
        for tree in &lock.git_trees {
            let (owner, name) = split_repository(&tree.repository)
                .ok_or_else(|| internal("private_tree_repository_invalid"))?;
            let archive = downloads.join(format!("tree-{}.tar.gz", tree.id));
            backend.download(
                &format!("repos/{owner}/{name}/tarball/{}", tree.commit),
                OCTET,
                &archive,
            )?;
            let destination = deps.join("git").join(&tree.id);
            extract_into(&archive, &destination, &mut budget)?;
            verified.push(VerifiedSource::tree(tree, &format!("git/{}", tree.id)));
            summary.trees += 1;
        }
    }
    Verified::new(verified).write(deps)?;
    Ok(summary)
}

fn read_json(
    backend: &dyn FetchBackend,
    route: &str,
    scratch: &Path,
) -> Result<Vec<u8>, OrchestratorError> {
    backend.download(route, JSON, scratch)?;
    let size = std::fs::metadata(scratch)
        .map_err(|err| OrchestratorError::io(scratch.display().to_string(), err.to_string()))?
        .len();
    if size > JSON_LIMIT {
        return Err(internal("private_api_json_too_large"));
    }
    std::fs::read(scratch)
        .map_err(|err| OrchestratorError::io(scratch.display().to_string(), err.to_string()))
}

/// Encode a tag for a URL path, keeping `/` (tags like `platform-history/2025-11`).
fn encode_tag(tag: &str) -> String {
    let mut out = String::with_capacity(tag.len());
    for byte in tag.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(UPPER_HEX[usize::from(byte >> 4)]));
            out.push(char::from(UPPER_HEX[usize::from(byte & 0x0f)]));
        }
    }
    out
}

fn fetch_asset(
    backend: &dyn FetchBackend,
    asset: &LockedAsset,
    destination: &Path,
    downloads: &Path,
    budget: &mut ArchiveBudget,
) -> Result<(), OrchestratorError> {
    let (owner, name) = split_repository(&asset.repository)
        .ok_or_else(|| internal("private_asset_repository_invalid"))?;
    let tag = encode_tag(&asset.tag);
    let scratch = downloads.join(format!("api-{}.json", asset.id));
    let commit = parse_commit(&read_json(
        backend,
        &format!("repos/{owner}/{name}/commits/refs/tags/{tag}"),
        &scratch,
    )?)?;
    if commit != asset.commit {
        return Err(internal(&format!("private_tag_moved:{}", asset.id)));
    }
    std::fs::remove_file(&scratch)
        .map_err(|err| OrchestratorError::io(scratch.display().to_string(), err.to_string()))?;
    let FoundAsset { id, sha256 } = parse_release_asset(
        &read_json(
            backend,
            &format!("repos/{owner}/{name}/releases/tags/{tag}"),
            &scratch,
        )?,
        &asset.asset,
    )?;
    if sha256
        .as_deref()
        .is_some_and(|published| published != asset.sha256)
    {
        return Err(internal(&format!(
            "private_asset_digest_changed:{}",
            asset.id
        )));
    }
    let archive = downloads.join(&asset.asset);
    backend.download(
        &format!("repos/{owner}/{name}/releases/assets/{id}"),
        OCTET,
        &archive,
    )?;
    let bytes = std::fs::read(&archive)
        .map_err(|err| OrchestratorError::io(archive.display().to_string(), err.to_string()))?;
    if crate::generator::sha256_hex(&bytes) != asset.sha256 {
        return Err(internal(&format!(
            "private_asset_sha256_mismatch:{}",
            asset.id
        )));
    }
    extract_into(&archive, destination, budget)
}

/// Run `private-fetch-v1` from the step environment.
///
/// # Errors
/// Any fetch failure; the credential directory is removed in every case.
pub fn run_private_fetch() -> Result<(), OrchestratorError> {
    let get = |key: &str| std::env::var(key).ok();
    let token =
        Token::new(&get(velnor_actions_contract::PRIVATE_FETCH_TOKEN_ENV).unwrap_or_default())?;
    let request = FetchRequest::parse(&get)?;
    let owned_env = owned_fetch_env(&request, &get)?;
    let session = Session::create(
        &request.runner_temp,
        token,
        &request.owner,
        &request.repositories,
        owned_env,
    )?;
    let summary = execute(&request, &session)?;
    drop(session);
    println!(
        "velnor: private sources fetched cargo_roots={} assets={} trees={}",
        summary.cargo_roots, summary.assets, summary.trees
    );
    Ok(())
}

#[cfg(test)]
#[path = "private_fetch_tests.rs"]
mod tests;
