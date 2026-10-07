//! Docs-lane inputs (`[docs]`): Bun app layout plus route smoke set.
//!
//! Presence opts the repository into the docs lane on any policy: the
//! lane installs the app with a frozen lockfile, lints MDX frontmatter,
//! typechecks, builds, validates absolute content links, and smokes the
//! configured routes against the build output. All paths are
//! app-relative spellings except `app_dir` itself (repo-relative); the
//! lane runs every step from `app_dir`.
use serde::{Deserialize, Serialize};
use velnor_actions_contract::errors::ContractError;

/// Docs-lane inputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DocsLaneConfig {
    /// Repo-relative app directory.
    pub app_dir: String,
    /// App-relative MDX collection directory (lint scope, link root).
    pub content_dir: String,
    /// Site base path serving `content_dir` (`/docs`).
    pub base_path: String,
    /// App-relative build output directory (route smoke root).
    pub output_dir: String,
    /// Sorted, duplicate-free absolute smoke routes.
    pub smoke_routes: Vec<String>,
}

impl DocsLaneConfig {
    /// Reference defaults: repo `docs/` app, fumadocs collection layout.
    #[must_use]
    pub fn defaults_for(base_path: &str) -> Self {
        Self {
            app_dir: "docs".to_owned(),
            content_dir: "content/docs".to_owned(),
            base_path: base_path.to_owned(),
            output_dir: ".output/public".to_owned(),
            smoke_routes: vec!["/".to_owned(), base_path.to_owned()],
        }
    }

    /// Validate every lane input; failures name file, key path, problem.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        check_repo_dir(file, "docs.app_dir", &self.app_dir)?;
        check_repo_dir(file, "docs.content_dir", &self.content_dir)?;
        check_repo_dir(file, "docs.output_dir", &self.output_dir)?;
        check_base_path(file, &self.base_path)?;
        if self.smoke_routes.is_empty() {
            return Err(ContractError::config(
                file,
                "docs.smoke_routes",
                "empty_routes",
            ));
        }
        let mut sorted = self.smoke_routes.clone();
        sorted.sort();
        if sorted != self.smoke_routes {
            return Err(ContractError::config(
                file,
                "docs.smoke_routes",
                "must_be_sorted",
            ));
        }
        for (index, route) in self.smoke_routes.iter().enumerate() {
            if index > 0 && self.smoke_routes[index - 1] == *route {
                return Err(ContractError::config(
                    file,
                    "docs.smoke_routes",
                    format!("duplicate_route:{route}"),
                ));
            }
            check_route(file, route)?;
        }
        Ok(())
    }
}

/// Repo-relative directory grammar: relative, no traversal, no empties.
fn check_repo_dir(file: &str, key: &str, value: &str) -> Result<(), ContractError> {
    if value.is_empty()
        || value.starts_with('/')
        || value.contains('\\')
        || value.split('/').any(|seg| seg.is_empty() || seg == "..")
        || !value.bytes().all(is_dir_byte)
    {
        return Err(ContractError::config(
            file,
            key,
            format!("malformed_dir:{value}"),
        ));
    }
    Ok(())
}

/// Bytes allowed in lane directories (paths plus dots for `.output`).
fn is_dir_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'-' | b'_')
}

/// Site base path: absolute, at least one segment, no trailing slash.
fn check_base_path(file: &str, value: &str) -> Result<(), ContractError> {
    if value.len() < 2
        || !value.starts_with('/')
        || value.ends_with('/')
        || value
            .split('/')
            .skip(1)
            .any(|seg| seg.is_empty() || seg == "..")
        || !value.bytes().all(is_route_byte)
    {
        return Err(ContractError::config(
            file,
            "docs.base_path",
            format!("malformed_base_path:{value}"),
        ));
    }
    Ok(())
}

/// One smoke route: absolute path over the route charset.
fn check_route(file: &str, route: &str) -> Result<(), ContractError> {
    if route == "/" {
        return Ok(());
    }
    if route.is_empty()
        || !route.starts_with('/')
        || route
            .split('/')
            .skip(1)
            .any(|seg| seg.is_empty() || seg == "..")
        || !route.bytes().all(is_route_byte)
    {
        return Err(ContractError::config(
            file,
            "docs.smoke_routes",
            format!("malformed_route:{route}"),
        ));
    }
    Ok(())
}

/// Bytes allowed in base paths and smoke routes.
fn is_route_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'-' | b'_' | b'~')
}

#[cfg(test)]
mod tests;
