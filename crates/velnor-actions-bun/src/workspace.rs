//! Workspace members, job identity and per-member script selection.

use std::collections::{BTreeMap, BTreeSet};

use velnor_actions_contract::config::{BUN_JOB_PREFIX, BunStackConfig, DEFAULT_BUN_SCRIPTS};
use velnor_actions_contract::matches_glob;

use crate::lockfile::BunLock;
use crate::package_json::PackageJson;
use crate::paths::under_root;
use crate::{BunError, parse_lock, parse_package_json};

/// One workspace member: the unit that owns a generated job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// `package.json` name.
    pub name: String,
    /// Workspace root the member belongs to (repository-relative, `""` for the root).
    pub root: String,
    /// Member key in `bun.lock` (directory relative to the root).
    pub key: String,
    /// Member directory, repository-relative (`""` for a single-package root).
    pub dir: String,
    /// Script names in the member's `package.json`.
    pub scripts: BTreeSet<String>,
    /// Names of the members this one depends on through `workspace:`.
    pub depends_on: BTreeSet<String>,
    /// Resolved `@playwright/test` (or `playwright`) version when the member depends on it.
    pub playwright: Option<String>,
}

/// The `playwright-core` package `bun.lock` resolves: the trust anchor of the browser lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaywrightCore {
    /// Resolved version.
    pub version: String,
    /// Tarball integrity recorded in the lock (`sha512-...`).
    pub integrity: String,
}

/// One workspace root with its members, sorted by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    /// Repository-relative root (`""` for the repository root).
    pub root: String,
    /// Root `packageManager`, when declared.
    pub package_manager: Option<String>,
    /// Members, sorted by name.
    pub members: Vec<Member>,
    /// Resolved `playwright-core`, when the lock holds it with an integrity.
    pub playwright_core: Option<PlaywrightCore>,
}

/// The member of `workspace` named `name`.
#[must_use]
pub fn resolve_member_dir<'a>(workspace: &'a Workspace, name: &str) -> Option<&'a Member> {
    workspace.members.iter().find(|member| member.name == name)
}

/// Slug of a package name: lowercase alphanumeric words joined by `-`.
#[must_use]
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            out.push(ch);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

/// Generated job id of a member (`bun-<slug>`).
#[must_use]
pub fn job_id(member: &Member) -> String {
    format!("{BUN_JOB_PREFIX}{}", slug(&member.name))
}

/// Display name of a member's job (`Bun / <name>`).
#[must_use]
pub fn job_name(member: &Member) -> String {
    format!("Bun / {}", member.name)
}

/// Resolve one workspace root from its text.
///
/// `members` maps each `bun.lock` workspace key to that member's
/// `package.json` text; the orchestrator reads exactly the keys the lock
/// lists. A root without `workspaces` is one single-package member.
///
/// # Errors
///
/// Fails closed with `bun_lock_stale` when the lock and the declared
/// patterns disagree, `bun_member_unnamed`/`bun_member_missing` for
/// incomplete members and `bun_member_name_mismatch` when the lock and the
/// member's `package.json` disagree on its name.
pub fn resolve(
    root: &str,
    root_package: &str,
    lock_text: &str,
    members: &BTreeMap<String, String>,
) -> Result<Workspace, BunError> {
    let root = if root == "." { "" } else { root };
    let package = parse_package_json(root_package)?;
    let lock = parse_lock(lock_text)?;
    let keys = member_keys(&package, &lock)?;
    let mut out = Vec::new();
    for key in &keys {
        let text = if key.is_empty() {
            root_package
        } else {
            members
                .get(key)
                .map(String::as_str)
                .ok_or_else(|| BunError::new("bun_member_missing", key))?
        };
        out.push(member_of(root, key, text, &lock)?);
    }
    link_dependencies(&mut out, &keys, &lock);
    out.sort_by(|left, right| left.name.cmp(&right.name));
    check_unique(&out)?;
    Ok(Workspace {
        root: root.to_owned(),
        package_manager: package.package_manager,
        members: out,
        playwright_core: playwright_core(&lock),
    })
}

fn playwright_core(lock: &BunLock) -> Option<PlaywrightCore> {
    Some(PlaywrightCore {
        version: lock.versions.get("playwright-core")?.clone(),
        integrity: lock.integrity.get("playwright-core")?.clone(),
    })
}

/// Lock keys that are members, validated against the declared patterns.
fn member_keys(package: &PackageJson, lock: &BunLock) -> Result<Vec<String>, BunError> {
    if package.workspaces.is_empty() {
        return Ok(vec![String::new()]);
    }
    let (negative, positive): (Vec<_>, Vec<_>) = package
        .workspaces
        .iter()
        .partition(|pattern| pattern.starts_with('!'));
    let declared = |key: &str| {
        positive.iter().any(|pattern| matches_glob(pattern, key))
            && !negative
                .iter()
                .any(|pattern| matches_glob(&pattern[1..], key))
    };
    let keys: Vec<String> = lock
        .workspaces
        .keys()
        .filter(|key| !key.is_empty())
        .cloned()
        .collect();
    if keys.iter().any(|key| !declared(key))
        || positive
            .iter()
            .any(|pattern| !keys.iter().any(|key| matches_glob(pattern, key)))
    {
        return Err(BunError::bare("bun_lock_stale"));
    }
    Ok(keys)
}

fn member_of(root: &str, key: &str, text: &str, lock: &BunLock) -> Result<Member, BunError> {
    let package = parse_package_json(text)?;
    let entry = lock
        .workspaces
        .get(key)
        .ok_or_else(|| BunError::bare("bun_lock_stale"))?;
    let name = package
        .name
        .clone()
        .ok_or_else(|| BunError::new("bun_member_unnamed", key))?;
    if entry.name.as_deref() != Some(name.as_str()) {
        return Err(BunError::new("bun_member_name_mismatch", &name));
    }
    let playwright = ["@playwright/test", "playwright"]
        .into_iter()
        .filter(|dep| entry.dependencies.contains_key(*dep))
        .find_map(|dep| lock.versions.get(dep).cloned());
    Ok(Member {
        name,
        root: root.to_owned(),
        key: key.to_owned(),
        dir: under_root(root, key),
        scripts: package.scripts,
        depends_on: BTreeSet::new(),
        playwright,
    })
}

fn link_dependencies(members: &mut [Member], keys: &[String], lock: &BunLock) {
    let names: BTreeSet<String> = members.iter().map(|member| member.name.clone()).collect();
    for (member, key) in members.iter_mut().zip(keys) {
        if let Some(entry) = lock.workspaces.get(key) {
            member.depends_on = entry
                .workspace_dependencies()
                .into_iter()
                .filter(|dep| names.contains(*dep) && *dep != member.name)
                .map(str::to_owned)
                .collect();
        }
    }
}

fn check_unique(members: &[Member]) -> Result<(), BunError> {
    let mut slugs = BTreeSet::new();
    for member in members {
        if !slugs.insert(slug(&member.name)) {
            return Err(BunError::new("bun_member_slug_collision", &member.name));
        }
    }
    Ok(())
}

/// Scripts one member runs, in execution order.
///
/// A member override must name scripts the member has. Otherwise the stack
/// list (or [`DEFAULT_BUN_SCRIPTS`]) is intersected with the member's own
/// scripts and keeps its order.
///
/// # Errors
///
/// Returns `bun_script_missing:<member>:<script>` for an override naming a
/// script the member lacks and `bun_member_no_scripts:<member>` when nothing runs.
pub fn member_scripts(member: &Member, config: &BunStackConfig) -> Result<Vec<String>, BunError> {
    let declared = config
        .members
        .get(&member.name)
        .and_then(|options| options.scripts.as_ref());
    let scripts: Vec<String> = if let Some(declared) = declared {
        if let Some(missing) = declared.iter().find(|name| !member.scripts.contains(*name)) {
            return Err(BunError::new(
                "bun_script_missing",
                &format!("{}:{missing}", member.name),
            ));
        }
        declared.clone()
    } else {
        let defaults: Vec<String> = if config.scripts.is_empty() {
            DEFAULT_BUN_SCRIPTS
                .iter()
                .map(|name| (*name).to_owned())
                .collect()
        } else {
            config.scripts.clone()
        };
        defaults
            .into_iter()
            .filter(|name| member.scripts.contains(name))
            .collect()
    };
    if scripts.is_empty() {
        return Err(BunError::new("bun_member_no_scripts", &member.name));
    }
    Ok(scripts)
}
