// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{ToolLock, ToolPin};

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
                label.ends_with("-13") || label.contains("intel") || label.contains("large");
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

impl ToolPin {
    #[must_use]
    pub fn applies_on(&self, request: &str, platform: MisePlatform<'_>) -> bool {
        self.os_filters
            .get(request)
            .is_none_or(|filters| filters.iter().any(|filter| platform.matches(filter)))
    }

    pub fn locked_on(&self, platform: MisePlatform<'_>) -> Result<Vec<&ToolLock>, String> {
        let mut found = Vec::new();
        for request in self
            .requested
            .iter()
            .filter(|request| self.applies_on(request, platform))
        {
            let candidates: Vec<_> =
                self.locked
                    .iter()
                    .filter(|row| {
                        row.os.as_ref().is_none_or(|filters| {
                            filters.iter().any(|filter| platform.matches(filter))
                        }) && if row.specifiers.is_empty() {
                            version_matches(request, &row.version)
                        } else {
                            row.specifiers.contains(request)
                        }
                    })
                    .collect();
            let ambiguous = candidates.iter().any(|row| {
                row.specifiers.is_empty()
                    && self
                        .requested
                        .iter()
                        .filter(|other| version_matches(other, &row.version))
                        .count()
                        > 1
            });
            if candidates.is_empty()
                || ambiguous
                || candidates
                    .iter()
                    .any(|row| row.version != candidates[0].version)
            {
                return Err(format!(
                    "request {request} has no unambiguous locked version; regenerate mise.lock with specifiers"
                ));
            }
            found.extend(candidates);
        }
        Ok(found)
    }
}

fn version_matches(request: &str, version: &str) -> bool {
    request == version
        || request == "latest"
        || version
            .strip_prefix(request)
            .is_some_and(|rest| rest.starts_with('.'))
}
