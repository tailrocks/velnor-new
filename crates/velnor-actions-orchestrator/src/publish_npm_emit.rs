//! Resolve `[[publish.npm]]` entries against the Bun inventory and the work tree.

use velnor_actions_bun::paths::under_root;
use velnor_actions_bun::{parse_package_json, resolve_member_dir};
use velnor_actions_contract::config::{NPM_PUBLISH_REGISTRY_URL, PublishNpm, VersionSource};
use velnor_actions_mise::ToolCatalog;

use crate::OrchestratorError;
use crate::config::CONFIG_REL;
use crate::prepare::GenerationPreparation;
use crate::publish_steps::{NpmPlan, VersionCheck};
use crate::safe_read::{MAX_REPO_FILE_BYTES, RepoRead, read_repo_file};
use crate::workflow_task_jobs::bun_steps::{bun_words, dir_arg, prepare_step};

fn failure(index: usize, field: &str, problem: &str) -> OrchestratorError {
    OrchestratorError::config(CONFIG_REL, format!("publish.npm[{index}].{field}"), problem)
}

fn read_text(
    prep: &GenerationPreparation,
    index: usize,
    rel: &str,
) -> Result<String, OrchestratorError> {
    match read_repo_file(&prep.root, rel, MAX_REPO_FILE_BYTES)? {
        RepoRead::Text(text) => Ok(text),
        RepoRead::Absent => Err(failure(index, "member", &format!("missing_file:{rel}"))),
    }
}

/// Resolve one entry: the member, its manifest policy and the Bun fragments.
///
/// # Errors
///
/// Fails closed when the member or root is not in `[stacks.bun]`, the member
/// is private or targets another registry, the pack script is missing, a
/// direct dependency comes from a private npm scope, or the version is not literal.
pub(crate) fn resolve<'a>(
    prep: &GenerationPreparation,
    index: usize,
    entry: &'a PublishNpm,
    catalog: &ToolCatalog,
) -> Result<NpmPlan<'a>, OrchestratorError> {
    let root = if entry.root == "." {
        ""
    } else {
        entry.root.as_str()
    };
    let workspace = prep
        .discovery
        .bun
        .roots
        .iter()
        .map(|bun_root| &bun_root.workspace)
        .find(|workspace| workspace.root == root)
        .ok_or_else(|| failure(index, "root", "not_a_stacks_bun_root"))?;
    let member = resolve_member_dir(workspace, &entry.member).ok_or_else(|| {
        failure(
            index,
            "member",
            &format!("unknown_bun_member:{}", entry.member),
        )
    })?;
    let manifest_path = under_root(&member.dir, "package.json");
    let text = read_text(prep, index, &manifest_path)?;
    let manifest = parse_package_json(&text).map_err(|err| failure(index, "member", err.code()))?;
    if manifest.private {
        return Err(failure(index, "member", "npm_member_private"));
    }
    if manifest.publish_registry.as_deref() != Some(NPM_PUBLISH_REGISTRY_URL) {
        return Err(failure(index, "member", "npm_publish_registry"));
    }
    let scopes = prep
        .config
        .private_sources
        .as_ref()
        .map_or(&[][..], |sources| sources.npm_scopes.as_slice());
    if let Some(dependency) = manifest.dependencies.keys().find(|name| {
        scopes
            .iter()
            .any(|scope| name.starts_with(&format!("{scope}/")))
    }) {
        return Err(failure(
            index,
            "member",
            &format!("npm_private_dependency_unsupported:{dependency}"),
        ));
    }
    let root_manifest =
        parse_package_json(&read_text(prep, index, &under_root(root, "package.json"))?)
            .map_err(|err| failure(index, "root", err.code()))?;
    if !root_manifest.scripts.contains(&entry.pack_script) {
        return Err(failure(index, "pack_script", "npm_pack_script_missing"));
    }
    if VersionSource::PackageJson.read(&text).is_none() {
        return Err(failure(index, "member", "version_not_literal"));
    }
    let words = |args: &[&str]| {
        let mut all: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        all.push(format!("--cwd={}", dir_arg(root)));
        bun_words(catalog, &all)
    };
    let pack_args = [
        "run".to_owned(),
        format!("--cwd={}", dir_arg(root)),
        entry.pack_script.clone(),
    ];
    Ok(NpmPlan {
        entry,
        check: VersionCheck::Manifest(VersionSource::PackageJson, manifest_path),
        install: words(&["install", "--frozen-lockfile"])?,
        pack: bun_words(catalog, &pack_args)?,
        prepare: prepare_step(catalog)?,
    })
}
