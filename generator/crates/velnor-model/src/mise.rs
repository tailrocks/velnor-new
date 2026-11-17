// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use crate::{ToolLock, ToolPin, ToolRequest};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MisePlatform<'a> {
    os: &'a str,
    arch: &'a str,
}

impl<'a> MisePlatform<'a> {
    #[must_use]
    pub fn new(os: &'a str, arch: &'a str) -> Self {
        Self {
            os: normalized_os(os),
            arch: normalized_arch(arch),
        }
    }

    #[must_use]
    pub fn matches(self, filter: &str) -> bool {
        let (os, arch) = filter
            .split_once('/')
            .map_or((filter, None), |(os, arch)| (os, Some(arch)));
        (normalized_os(os) == self.os || (os == "unix" && self.os != "windows"))
            && arch.is_none_or(|arch| normalized_arch(arch) == self.arch)
    }

    #[must_use]
    pub fn lock_key(self) -> String {
        format!("{}-{}", self.os, self.arch)
    }
}

impl MisePlatform<'static> {
    #[must_use]
    pub fn runner(label: &str) -> Option<Self> {
        let label = label.to_ascii_lowercase();
        let arm = label.contains("arm");
        if label.starts_with("ubuntu") || label.starts_with("linux") {
            Some(Self::new("linux", if arm { "arm64" } else { "x64" }))
        } else if label.starts_with("macos") {
            let intel =
                label.ends_with("-13") || label.ends_with("-intel") || label.ends_with("-large");
            Some(Self::new("macos", if intel { "x64" } else { "arm64" }))
        } else if label.starts_with("windows") {
            Some(Self::new("windows", if arm { "arm64" } else { "x64" }))
        } else {
            None
        }
    }
}

fn normalized_os(os: &str) -> &str {
    match os {
        "darwin" => "macos",
        "win" => "windows",
        other => other,
    }
}

fn normalized_arch(arch: &str) -> &str {
    match arch {
        "aarch64" => "arm64",
        "x86_64" | "amd64" => "x64",
        other => other,
    }
}

impl ToolRequest {
    #[must_use]
    pub fn applies_on(&self, platform: MisePlatform<'_>) -> bool {
        self.os
            .as_ref()
            .is_none_or(|filters| filters.iter().any(|filter| platform.matches(filter)))
    }
}

impl ToolPin {
    pub fn locked_on(&self, platform: MisePlatform<'_>) -> Result<Vec<&ToolLock>, String> {
        let mut found = Vec::new();
        for request in self
            .requested
            .iter()
            .filter(|request| request.applies_on(platform))
        {
            let candidates: Vec<_> = self
                .locked
                .iter()
                .filter(|row| {
                    options_match(request, row)
                        && row.os.as_ref().is_none_or(|filters| {
                            filters.iter().any(|filter| platform.matches(filter))
                        })
                        && if row.specifiers.is_empty() {
                            version_matches(&request.version, &row.version)
                        } else {
                            row.specifiers.contains(&request.version)
                        }
                })
                .collect();
            let ambiguous = candidates.iter().any(|row| {
                row.specifiers.is_empty()
                    && self
                        .requested
                        .iter()
                        .filter(|other| {
                            options_match(other, row)
                                && version_matches(&other.version, &row.version)
                        })
                        .map(|other| &other.version)
                        .collect::<BTreeSet<_>>()
                        .len()
                        > 1
            });
            if candidates.len() != 1 || ambiguous {
                return Err(format!(
                    "request {} has no unambiguous locked version; regenerate mise.lock with specifiers",
                    request.version
                ));
            }
            found.extend(candidates);
        }
        Ok(found)
    }
}

fn options_match(request: &ToolRequest, row: &ToolLock) -> bool {
    let packslip = row
        .backend
        .as_deref()
        .is_some_and(|backend| backend.starts_with("packslip:"));
    request
        .options
        .iter()
        .filter(|(key, _)| !(packslip && key.as_str() == "workflow"))
        .eq(row.options.iter())
}

fn version_matches(request: &str, version: &str) -> bool {
    request == version
        || request == "latest"
        || version
            .strip_prefix(request)
            .is_some_and(|rest| rest.starts_with('.'))
}
