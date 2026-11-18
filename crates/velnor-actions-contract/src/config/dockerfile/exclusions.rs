// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::DockerfileFacts;

impl DockerfileFacts {
    #[must_use]
    pub fn source_excludes_path(&self, source: &str, path: &str) -> bool {
        let relative = if source == "." {
            path
        } else if source == path {
            path.rsplit('/').next().unwrap_or(path)
        } else if let Some(relative) = path.strip_prefix(source).and_then(|s| s.strip_prefix('/')) {
            relative
        } else {
            return false;
        };
        self.context_source_excludes
            .get(source)
            .is_some_and(|sets| {
                !sets.is_empty()
                    && sets.iter().all(|patterns| {
                        patterns.iter().any(|pattern| {
                            let first = relative.split('/').next().unwrap_or(relative);
                            if let Some(suffix) = pattern.strip_prefix('*') {
                                first.ends_with(suffix)
                            } else {
                                first == pattern
                            }
                        })
                    })
            })
    }
}
