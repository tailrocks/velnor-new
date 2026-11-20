// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{RepoPath, RepoView, RepositoryIdentity, RustCiConfig, RustRoot};

use crate::error::{DetectError, Reason};

use crate::roots::{CARGO_LOCK, is_cargo_config};
use crate::toml_file::table;
use crate::weaken_scope::reaches_a_check;

const PUBLIC_SOURCES: [&str; 2] = [
    "registry+https://github.com/rust-lang/crates.io-index",
    "sparse+https://index.crates.io/",
];

/// The note that explains why the base caches are not saved.
pub(crate) fn note(cause: &str) -> String {
    format!("base caches are not saved because this repository has private dependencies ({cause})")
}

/// The first reason the repository's dependencies are not proven public.
pub(crate) fn private_cause(view: &RepoView, roots: &[RustRoot]) -> Option<String> {
    let locks = roots
        .iter()
        .map(|root| root.manifest.sibling(&CARGO_LOCK))
        .filter_map(|lock| {
            view.files
                .get(&lock)
                .map(|bytes| locked_source(&lock, bytes))
        })
        .flatten();
    let configs = view
        .files
        .iter()
        .filter(|(path, _)| is_cargo_config(path) && reaches_a_check(path, roots))
        .filter_map(|(path, bytes)| configured_source(path, bytes));
    locks.chain(configs).next()
}

pub(crate) fn refuse_locked_credentials(
    view: &RepoView,
    config: &RustCiConfig,
    roots: &[RustRoot],
) -> Result<(), DetectError> {
    if config.private_sources.is_none() {
        return Ok(());
    }
    match locked_credentials(view, roots) {
        Some((lock, name)) => Err(DetectError::invalid(
            &lock,
            Reason::LockedUrlCredentials(name),
        )),
        None => Ok(()),
    }
}

fn locked_credentials(view: &RepoView, roots: &[RustRoot]) -> Option<(RepoPath, String)> {
    roots.iter().find_map(|root| {
        let lock = root.manifest.sibling(&CARGO_LOCK);
        let document = table(view.files.get(&lock)?).ok()?;
        let name = document
            .get("package")
            .and_then(toml::Value::as_array)?
            .iter()
            .filter_map(toml::Value::as_table)
            .find_map(|package| {
                let source = package.get("source").and_then(toml::Value::as_str)?;
                let (scheme, tail) = source.strip_prefix("git+")?.split_once("://")?;
                let authority = tail.split('/').next().unwrap_or("");
                (scheme != "ssh" && authority.contains('@'))
                    .then(|| package.get("name").and_then(toml::Value::as_str))?
            })?;
        Some((lock, name.to_owned()))
    })
}

fn locked_source(lock: &RepoPath, bytes: &[u8]) -> Option<String> {
    let Ok(document) = table(bytes) else {
        return Some(format!("{lock} is unreadable"));
    };
    if document.contains_key("patch") {
        return Some(format!(
            "{lock} has patches whose sources are not proven public"
        ));
    }
    document
        .get("package")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(toml::Value::as_table)
        .find_map(|package| {
            let source = package.get("source").and_then(toml::Value::as_str)?;
            let name = package.get("name").and_then(toml::Value::as_str)?;
            (!PUBLIC_SOURCES.contains(&source))
                .then(|| format!("{lock} locks {name} from {source}"))
        })
}

fn configured_source(path: &RepoPath, bytes: &[u8]) -> Option<String> {
    let Ok(document) = table(bytes) else {
        return Some(format!("{path} is unreadable"));
    };
    ["registries", "source"]
        .into_iter()
        .find(|section| {
            document
                .get(*section)
                .and_then(toml::Value::as_table)
                .is_some_and(|entries| !entries.is_empty())
        })
        .map(|section| format!("{path} declares [{section}]"))
}

pub fn cacheable_lock(text: &str) -> Result<bool, crate::lock::LockError> {
    let lock = crate::cargo_lock::CargoLock::parse(text)?;
    Ok(lock.patch.is_none()
        && lock.packages.iter().all(|package| {
            package
                .source
                .as_deref()
                .is_none_or(|source| PUBLIC_SOURCES.contains(&source))
        }))
}

pub fn cacheable_config(text: &str) -> Result<bool, crate::lock::LockError> {
    let document: toml::Table = toml::from_str(text)
        .map_err(|error| crate::lock::LockError::new(error.message().to_owned()))?;
    Ok(![
        "source",
        "registries",
        "registry",
        "include",
        "patch",
        "paths",
    ]
    .iter()
    .any(|key| document.contains_key(*key)))
}

const PUBLIC_GIT_HOSTS: [&str; 4] = ["github.com", "gitlab.com", "codeberg.org", "bitbucket.org"];

pub(crate) fn private_suggestion(
    view: &RepoView,
    config: &RustCiConfig,
    roots: &[RustRoot],
) -> Option<String> {
    if let Some(sources) = &config.private_sources {
        return crate::private_lock::mismatch(view, sources, roots);
    }
    let owner = match &view.repository {
        RepositoryIdentity::Named(name) => name.split('/').next().filter(|_| name.contains('/')),
        RepositoryIdentity::Unhosted => None,
    };
    let looking: Vec<(RepoPath, String, String)> =
        crate::private_lock::locked_packages(view, roots)
            .into_iter()
            .filter(|(_, _, source)| private_looking(source, owner))
            .collect();
    if let Some((lock, name, _)) = looking
        .iter()
        .find(|(_, _, source)| crate::private_lock::github_repository(source).is_some())
    {
        return Some(format!(
            "{lock} locks {name} from a source that looks private; to build it, add a [private_sources] table to .velnor/rust-ci.toml with app_id = 'vars.<NAME>', private_key = 'secrets.<NAME>' and repositories = ['owner/name'] (see CONFIG.md)"
        ));
    }
    looking.first().map(|(lock, name, _)| {
        format!(
            "{lock} locks {name} from a source that looks private but is not a github.com repository; [private_sources] creates a GitHub App token for repositories on the workflow's GitHub server only, so it cannot authenticate this source unless GitHub Enterprise Server hosts it, and a cold fetch needs its credentials another way"
        )
    })
}

fn private_looking(source: &str, owner: Option<&str>) -> bool {
    if PUBLIC_SOURCES.contains(&source) {
        return false;
    }
    let Some(rest) = source.strip_prefix("git+") else {
        return source.starts_with("registry+") || source.starts_with("sparse+");
    };
    let Some((scheme, tail)) = rest.split_once("://") else {
        return true;
    };
    let authority = tail.split('/').next().unwrap_or("");
    if scheme != "ssh" && authority.contains('@') {
        return false;
    }
    let credentialed = scheme == "ssh";
    let host = authority.rsplit('@').next().unwrap_or("");
    credentialed || !PUBLIC_GIT_HOSTS.contains(&host) || same_owner(tail, owner)
}

fn same_owner(tail: &str, owner: Option<&str>) -> bool {
    owner.is_some_and(|owner| {
        tail.split('/')
            .nth(1)
            .is_some_and(|first| first.eq_ignore_ascii_case(owner))
    })
}
