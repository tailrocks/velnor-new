// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

const FMT_SUFFIXES: [&str; 5] = [".tofutest.hcl", ".tftest.hcl", ".tfvars", ".tofu", ".tf"];

#[must_use]
pub fn is_excluded_name(file_name: &str) -> bool {
    file_name.starts_with('.')
        || file_name.starts_with('~')
        || (file_name.starts_with('#') && file_name.ends_with('#'))
}

#[must_use]
pub fn is_fmt_file(file_name: &str) -> bool {
    if is_excluded_name(file_name) {
        return false;
    }
    FMT_SUFFIXES.iter().any(|suffix| {
        file_name
            .strip_suffix(suffix)
            .is_some_and(|stem| !stem.is_empty())
    })
}

#[must_use]
pub fn under_hidden_dir(path: &str) -> bool {
    let mut segments: Vec<&str> = path.split('/').collect();
    segments.pop();
    segments.iter().any(|segment| segment.starts_with('.'))
}

#[must_use]
pub fn fmt_set(paths: &[String]) -> Vec<String> {
    let mut selected: Vec<String> = paths
        .iter()
        .filter(|path| {
            let name = path.rsplit('/').next().unwrap_or(path);
            is_fmt_file(name) && !under_hidden_dir(path)
        })
        .cloned()
        .collect();
    selected.sort();
    selected
}

#[must_use]
pub fn fmt_scope_for_root(paths: &[String], prefix: &str) -> Vec<String> {
    fmt_set(paths)
        .into_iter()
        .filter(|path| prefix.is_empty() || path.starts_with(&format!("{prefix}/")))
        .collect()
}

#[must_use]
pub fn covered_fmt_roots(roots: &[String]) -> std::collections::BTreeSet<String> {
    let mut covered = std::collections::BTreeSet::new();
    for root in roots {
        let dominated = roots.iter().any(|other| {
            other != root && (other.is_empty() || root.starts_with(&format!("{other}/")))
        });
        if dominated {
            covered.insert(root.clone());
        }
    }
    covered
}
