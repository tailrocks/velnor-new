// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub const PROJECT_TOOL_REF_PREFIX: &str = "${VELNOR_TOOL_";

pub const PROJECT_TOOLS_STEP_NAME: &str = "Resolve project tool versions";

pub const PROJECT_MISE_CONFIG_PATTERNS: [&str; 13] = [
    "mise.toml",
    ".mise.toml",
    "mise.*.toml",
    ".mise.*.toml",
    ".config/mise.toml",
    ".config/mise.*.toml",
    ".config/mise/config.toml",
    ".config/mise/config.*.toml",
    ".config/mise/conf.d/*.toml",
    "mise/config.toml",
    "mise/config.*.toml",
    ".mise/config.toml",
    ".mise/config.*.toml",
];

pub const PROJECT_TOOL_VERSIONS_FILE: &str = ".tool-versions";

pub const RUST_TOOLCHAIN_SOURCE_FILES: [&str; 2] = ["rust-toolchain.toml", "rust-toolchain"];

pub const PROJECT_TOOL_LOCK_FILES: [&str; 2] = ["mise.lock", ".mise.lock"];

#[must_use]
pub fn matches_source_pattern(pattern: &str, path: &str) -> bool {
    let (pattern_dir, pattern_name) = pattern.rsplit_once('/').unwrap_or(("", pattern));
    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
    if dir != pattern_dir {
        return false;
    }
    match pattern_name.split_once('*') {
        None => name == pattern_name,
        Some((prefix, suffix)) => {
            name.len() >= prefix.len() + suffix.len()
                && name.starts_with(prefix)
                && name.ends_with(suffix)
        }
    }
}

#[must_use]
pub fn is_declaration_source(path: &str) -> bool {
    path == PROJECT_TOOL_VERSIONS_FILE
        || RUST_TOOLCHAIN_SOURCE_FILES.contains(&path)
        || PROJECT_MISE_CONFIG_PATTERNS
            .iter()
            .any(|pattern| matches_source_pattern(pattern, path))
}

#[must_use]
pub fn is_project_tool_lock(path: &str) -> bool {
    PROJECT_TOOL_LOCK_FILES.contains(&path)
}

pub const PROJECT_TOOL_LOCK_SUFFIX: &str = "_LOCK";

#[must_use]
pub fn project_tool_lock_name(name: &str) -> String {
    format!("{name}{PROJECT_TOOL_LOCK_SUFFIX}")
}

#[must_use]
pub fn project_tool_lock_base(name: &str) -> Option<&str> {
    name.strip_suffix(PROJECT_TOOL_LOCK_SUFFIX)
        .filter(|base| !crate::config::project_tool_resolver::project_tool_keys(base).is_empty())
}

#[must_use]
pub fn tools_identity_input<'a>(
    static_digest: &str,
    names: impl IntoIterator<Item = &'a str>,
) -> String {
    let mut names: Vec<&str> = names.into_iter().collect();
    names.sort_unstable();
    names.dedup();
    let mut input = static_digest.to_owned();
    for name in names {
        for part in ["-${", name, "}-${", name, PROJECT_TOOL_LOCK_SUFFIX, "}"] {
            input.push_str(part);
        }
    }
    input
}

#[must_use]
pub fn is_tools_identity_input(input: &str) -> bool {
    let Some(digest) = input.get(..64) else {
        return false;
    };
    if !digest
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return false;
    }
    let names: Vec<&str> = project_tool_ref_names(&input[64..])
        .into_iter()
        .filter(|name| project_tool_lock_base(name).is_none())
        .collect();
    names
        .iter()
        .all(|name| !crate::config::project_tool_resolver::project_tool_keys(name).is_empty())
        && tools_identity_input(digest, names) == input
}

fn ref_spans(text: &str) -> Vec<(usize, usize, &str)> {
    let mut spans = Vec::new();
    let mut offset = 0;
    while let Some(found) = text[offset..].find(PROJECT_TOOL_REF_PREFIX) {
        let start = offset + found;
        let name_start = start + 2;
        let rest = &text[name_start..];
        let name_len = rest
            .bytes()
            .take_while(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_')
            .count();
        let name = &rest[..name_len];
        if name.len() > "VELNOR_TOOL_".len() && rest[name_len..].starts_with('}') {
            let end = name_start + name_len + 1;
            spans.push((start, end, name));
            offset = end;
        } else {
            offset = start + PROJECT_TOOL_REF_PREFIX.len();
        }
    }
    spans
}

#[must_use]
pub fn is_exact_or_project_version(version: &str) -> bool {
    is_project_tool_ref(version) || is_exact_project_version(version)
}

#[must_use]
pub fn is_exact_project_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (*part == "0" || !part.starts_with('0'))
        })
}

#[must_use]
pub fn is_project_tool_ref(value: &str) -> bool {
    matches!(ref_spans(value).as_slice(), [(0, end, _)] if *end == value.len())
}

#[must_use]
pub fn has_project_tool_ref(text: &str) -> bool {
    !ref_spans(text).is_empty()
}

#[must_use]
pub fn project_tool_ref_names(text: &str) -> Vec<&str> {
    ref_spans(text)
        .into_iter()
        .map(|(_, _, name)| name)
        .collect()
}

#[must_use]
pub fn map_project_tool_refs(text: &str, replace: impl Fn(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut offset = 0;
    for (start, end, name) in ref_spans(text) {
        out.push_str(&text[offset..start]);
        out.push_str(&replace(name));
        offset = end;
    }
    out.push_str(&text[offset..]);
    out
}

#[must_use]
pub fn project_tool_refs_to_expressions(text: &str) -> String {
    map_project_tool_refs(text, |name| format!("${{{{ env.{name} }}}}"))
}

#[must_use]
pub fn without_project_tool_refs(text: &str) -> String {
    map_project_tool_refs(text, |_| "0".to_owned())
}
