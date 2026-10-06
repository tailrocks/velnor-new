//! Official release evidence alone never grants executed-workflow authority.

use std::ffi::OsString;
use std::path::Path;

use velnor_actions_contract::{
    EXPECTED_REPOSITORY as REPOSITORY, PlanGenerator, RELEASE_MANIFEST_FILENAME as MANIFEST_NAME,
    ReleaseManifest, SUPPORTED_TARGETS, parse_strict_json,
};
use velnor_actions_mise::{RuntimePaths, ToolCatalog};

use crate::cover::shard_baseline::BaselineLookup;

/// Only authenticated, immutable GitHub service responses construct this value.
struct AuthenticatedGeneratorRelease {
    manifest: ReleaseManifest,
}

impl AuthenticatedGeneratorRelease {
    /// Retrieve the compiled version's official immutable manifest and asset digests.
    fn retrieve(root: &Path, runtime: RuntimePaths) -> Result<Self, String> {
        let version = env!("CARGO_PKG_VERSION");
        let catalog = ToolCatalog::pinned();
        let release_text = api(
            &catalog,
            root,
            runtime,
            &format!("repos/{REPOSITORY}/releases/tags/v{version}"),
            false,
        )?;
        let release = parse_strict_json(&release_text)
            .map_err(|_| "generator_release_response_invalid".to_owned())?;
        check_release(&release, version)?;
        let asset = asset_named(&release, MANIFEST_NAME)?;
        let id = asset["id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or_else(|| "generator_release_asset_invalid".to_owned())?;
        let expected = asset_digest(asset)?;
        let text = api(
            &catalog,
            root,
            runtime,
            &format!("repos/{REPOSITORY}/releases/assets/{id}"),
            true,
        )?;
        let source = resolve_tag(&catalog, root, runtime, version)?;
        Self::from_service(&release, &text, &expected, version, &source)
    }

    /// Bind both binaries independently; a serialized planning tuple grants nothing.
    fn bind_pair(&self, planned: &PlanGenerator, actual: &PlanGenerator) -> Result<(), String> {
        for identity in [planned, actual] {
            if identity.version != self.manifest.version
                || !self
                    .manifest
                    .record_for_target(&identity.target)
                    .is_some_and(|record| record.sha256 == identity.sha256)
            {
                return Err("generator_release_binary_mismatch".to_owned());
            }
        }
        Ok(())
    }

    fn authorize_execution(
        &self,
        planned: &PlanGenerator,
        actual: &PlanGenerator,
    ) -> Result<PlanGenerator, String> {
        self.bind_pair(planned, actual)?;
        // Neither compatibility/report ABI qualification nor actual executed
        // workflow acquisition is represented by the schema-1 release manifest.
        Err("generator_executed_workflow_acquisition_missing".to_owned())
    }

    fn from_service(
        release: &serde_json::Value,
        text: &str,
        expected_digest: &str,
        version: &str,
        source_commit: &str,
    ) -> Result<Self, String> {
        check_release(release, version)?;
        let asset = asset_named(release, MANIFEST_NAME)?;
        if asset["browser_download_url"]
            != format!(
                "https://github.com/{REPOSITORY}/releases/download/v{version}/{MANIFEST_NAME}"
            )
        {
            return Err("generator_release_manifest_url_mismatch".to_owned());
        }
        if asset_digest(asset)? != expected_digest
            || crate::cover_identity::generator::sha256_hex(text.as_bytes()) != expected_digest
        {
            return Err("generator_release_manifest_digest_mismatch".to_owned());
        }
        let manifest =
            ReleaseManifest::parse_json(text, MANIFEST_NAME).map_err(|error| error.to_string())?;
        manifest
            .validate(MANIFEST_NAME)
            .map_err(|error| error.to_string())?;
        if manifest.version != version || source_commit != manifest.commit {
            return Err("generator_release_source_mismatch".to_owned());
        }
        if manifest.targets.len() != SUPPORTED_TARGETS.len()
            || SUPPORTED_TARGETS
                .iter()
                .any(|target| manifest.record_for_target(target).is_none())
        {
            return Err("generator_release_targets_incomplete".to_owned());
        }
        for record in &manifest.targets {
            let name = record
                .artifact
                .rsplit('/')
                .next()
                .ok_or_else(|| "generator_release_asset_invalid".to_owned())?;
            let binary = asset_named(release, name)?;
            if binary["browser_download_url"] != record.artifact
                || asset_digest(binary)? != record.sha256
            {
                return Err("generator_release_asset_digest_mismatch".to_owned());
            }
        }
        Ok(Self { manifest })
    }
}

/// Resolve the actual immutable release tag; release target_commitish is only a hint.
fn resolve_tag(
    catalog: &ToolCatalog,
    root: &Path,
    runtime: RuntimePaths,
    version: &str,
) -> Result<String, String> {
    resolve_tag_with(version, |endpoint| {
        api(catalog, root, runtime, endpoint, false)
    })
}

fn resolve_tag_with(
    version: &str,
    mut read: impl FnMut(&str) -> Result<String, String>,
) -> Result<String, String> {
    let endpoint = format!("repos/{REPOSITORY}/git/ref/tags/v{version}");
    let text = read(&endpoint)?;
    let value = parse_strict_json(&text).map_err(|_| "generator_release_tag_invalid".to_owned())?;
    if value["ref"] != format!("refs/tags/v{version}") {
        return Err("generator_release_tag_invalid".to_owned());
    }
    let mut object = value["object"].clone();
    let mut visited = std::collections::BTreeSet::new();
    for _ in 0..4 {
        let sha = object["sha"]
            .as_str()
            .filter(|sha| {
                sha.len() == 40
                    && sha.bytes().any(|byte| byte != b'0')
                    && sha
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            })
            .ok_or_else(|| "generator_release_tag_invalid".to_owned())?;
        if !visited.insert(sha.to_owned()) {
            return Err("generator_release_tag_invalid".to_owned());
        }
        match object["type"].as_str() {
            Some("commit") => return Ok(sha.to_owned()),
            Some("tag") => {
                let text = read(&format!("repos/{REPOSITORY}/git/tags/{sha}"))?;
                let tag = parse_strict_json(&text)
                    .map_err(|_| "generator_release_tag_invalid".to_owned())?;
                if tag["sha"] != sha {
                    return Err("generator_release_tag_invalid".to_owned());
                }
                object = tag["object"].clone();
            }
            _ => return Err("generator_release_tag_invalid".to_owned()),
        }
    }
    Err("generator_release_tag_depth_exceeded".to_owned())
}

/// Cross-host proof stays closed until executed workflow acquisition is authenticated.
/// Release membership proves paired artifacts, never which planner a run executed.
pub(crate) fn authenticate(root: &Path, planned: &PlanGenerator) -> Result<PlanGenerator, String> {
    let actual = crate::internal_plan::default_generator();
    let release = AuthenticatedGeneratorRelease::retrieve(root, RuntimePaths::full())?;
    release.authorize_execution(planned, &actual)
}

fn check_release(release: &serde_json::Value, version: &str) -> Result<(), String> {
    if release["id"].as_u64().is_none_or(|id| id == 0)
        || release["tag_name"] != format!("v{version}")
        || release["draft"] != false
        || release["prerelease"] != false
        || release["immutable"] != true
        || release["published_at"].as_str().is_none_or(str::is_empty)
        || release["html_url"] != format!("https://github.com/{REPOSITORY}/releases/tag/v{version}")
    {
        return Err("generator_release_not_immutable_official".to_owned());
    }
    let assets = release["assets"]
        .as_array()
        .ok_or_else(|| "generator_release_assets_missing".to_owned())?;
    let mut ids = std::collections::BTreeSet::new();
    for asset in assets {
        let id = asset["id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or_else(|| "generator_release_asset_invalid".to_owned())?;
        if !ids.insert(id) {
            return Err("generator_release_asset_invalid".to_owned());
        }
    }
    Ok(())
}

fn asset_named<'a>(
    release: &'a serde_json::Value,
    name: &str,
) -> Result<&'a serde_json::Value, String> {
    let assets = release["assets"]
        .as_array()
        .ok_or_else(|| "generator_release_assets_missing".to_owned())?;
    let mut matching = assets.iter().filter(|asset| asset["name"] == name);
    let asset = matching
        .next()
        .ok_or_else(|| "generator_release_asset_missing".to_owned())?;
    if matching.next().is_some()
        || asset["state"] != "uploaded"
        || asset["size"].as_u64().is_none_or(|size| size == 0)
    {
        return Err("generator_release_asset_invalid".to_owned());
    }
    Ok(asset)
}

fn asset_digest(asset: &serde_json::Value) -> Result<String, String> {
    asset["digest"]
        .as_str()
        .and_then(|value| value.strip_prefix("sha256:"))
        .filter(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                && value.bytes().any(|byte| byte != b'0')
        })
        .map(str::to_owned)
        .ok_or_else(|| "generator_release_asset_digest_missing".to_owned())
}

fn api(
    catalog: &ToolCatalog,
    root: &Path,
    runtime: RuntimePaths,
    endpoint: &str,
    raw: bool,
) -> Result<String, String> {
    let mut args = vec![
        OsString::from("api"),
        OsString::from("--hostname"),
        OsString::from("github.com"),
    ];
    if raw {
        args.extend([
            OsString::from("--header"),
            OsString::from("Accept: application/octet-stream"),
        ]);
    }
    args.push(OsString::from(endpoint));
    BaselineLookup::run_in_runtime(catalog, root, args, runtime)
}

#[cfg(test)]
#[path = "generator_runtime_release_tests.rs"]
mod tests;
