//! Run-time references to project-declared tool versions (hard decision 5).
//!
//! Generated text never copies a version the project's Mise config
//! declares. It carries `${VELNOR_TOOL_<NAME>}` instead (`<NAME>` is
//! `[A-Z0-9_]+`), which the generated resolver step fills from the project
//! config at run time. Shell text and task argv keep the shell form;
//! `with:`/`env:` mappings and cache keys take the expression form
//! `${{ env.VELNOR_TOOL_<NAME> }}`.

/// Shell-form prefix of every project reference.
pub const PROJECT_TOOL_REF_PREFIX: &str = "${VELNOR_TOOL_";

/// Contract-fixed display name of the generated resolver step.
pub const PROJECT_TOOLS_STEP_NAME: &str = "Resolve project tool versions";

/// Every location Mise reads `[tools]` from in a project root.
///
/// One entry is `dir/name`; `name` may hold one `*` that matches any
/// characters except `/`. This covers the plain, dot-prefixed and
/// `.config` spellings, `conf.d`, environment files (`mise.<env>.toml`)
/// and `mise.local.toml`. The generator reads exactly this list, the
/// generated resolver globs exactly this list, and change selection
/// treats exactly this list as declaration sources.
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

/// Mise's asdf-compatible tool list.
pub const PROJECT_TOOL_VERSIONS_FILE: &str = ".tool-versions";

/// Rust toolchain files Mise (idiomatic version files) and rustup both read.
pub const RUST_TOOLCHAIN_SOURCE_FILES: [&str; 2] = ["rust-toolchain.toml", "rust-toolchain"];

/// Mise lock files. Change selection re-tests everything on any change;
/// the resolver reads each installed tool's rows for the tools-cache identity.
pub const PROJECT_TOOL_LOCK_FILES: [&str; 2] = ["mise.lock", ".mise.lock"];

/// Whether `path` matches one `dir/name` pattern of
/// [`PROJECT_MISE_CONFIG_PATTERNS`].
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

/// Whether `path` is a file a project tool version may be declared in.
#[must_use]
pub fn is_declaration_source(path: &str) -> bool {
    path == PROJECT_TOOL_VERSIONS_FILE
        || RUST_TOOLCHAIN_SOURCE_FILES.contains(&path)
        || PROJECT_MISE_CONFIG_PATTERNS
            .iter()
            .any(|pattern| matches_source_pattern(pattern, path))
}

/// Whether `path` is a lock file that always re-tests everything.
#[must_use]
pub fn is_project_tool_lock(path: &str) -> bool {
    PROJECT_TOOL_LOCK_FILES.contains(&path)
}

/// Suffix of the variable carrying one project tool's `mise.lock` digest.
///
/// `${VELNOR_TOOL_<NAME>_LOCK}` is the SHA-256 of that tool's lock
/// `checksum`/`url` entries for the runner platform (or `unlocked`), which
/// the resolver step exports next to `VELNOR_TOOL_<NAME>`.
pub const PROJECT_TOOL_LOCK_SUFFIX: &str = "_LOCK";

/// Lock-digest variable of the tool behind `name`.
#[must_use]
pub fn project_tool_lock_name(name: &str) -> String {
    format!("{name}{PROJECT_TOOL_LOCK_SUFFIX}")
}

/// Tool variable a lock-digest variable belongs to, or `None` for a
/// version variable.
#[must_use]
pub fn project_tool_lock_base(name: &str) -> Option<&str> {
    name.strip_suffix(PROJECT_TOOL_LOCK_SUFFIX)
        .filter(|base| !crate::config::project_tool_resolver::project_tool_keys(base).is_empty())
}

/// `d` input of the tools-cache identity: the static digest, then for each
/// project-declared tool the job installs (sorted) its resolved version and
/// its lock digest, both read from the resolver step's `env` exports.
///
/// Only the tools the job installs enter the identity: an unrelated Mise
/// task, comment, or another tool's pin or lock entry never changes it.
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

/// Valid `d` input: a lower-case SHA-256 static digest followed by exactly
/// the [`tools_identity_input`] references of catalog tools.
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

/// Byte span `(start, end, name)` of each project reference in `text`.
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

/// Exact numeric `X.Y.Z` pin, or one project reference the generated
/// resolver validates as exact at run time.
#[must_use]
pub fn is_exact_or_project_version(version: &str) -> bool {
    is_project_tool_ref(version) || is_exact_project_version(version)
}

/// Exact `X.Y.Z` of plain decimal numbers without leading zeros: the one
/// shape the generated resolver accepts for a project-declared version.
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

/// True when `value` is exactly one project reference.
#[must_use]
pub fn is_project_tool_ref(value: &str) -> bool {
    matches!(ref_spans(value).as_slice(), [(0, end, _)] if *end == value.len())
}

/// True when `text` carries at least one project reference.
#[must_use]
pub fn has_project_tool_ref(text: &str) -> bool {
    !ref_spans(text).is_empty()
}

/// Environment variable names referenced by `text`, in order.
#[must_use]
pub fn project_tool_ref_names(text: &str) -> Vec<&str> {
    ref_spans(text)
        .into_iter()
        .map(|(_, _, name)| name)
        .collect()
}

/// Replace every project reference with `replace(name)`.
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

/// Rewrite every project reference to its `${{ env.NAME }}` expression form.
#[must_use]
pub fn project_tool_refs_to_expressions(text: &str) -> String {
    map_project_tool_refs(text, |name| format!("${{{{ env.{name} }}}}"))
}

/// Replace every project reference with `0`, so literal character-class
/// validators can check the surrounding text.
#[must_use]
pub fn without_project_tool_refs(text: &str) -> String {
    map_project_tool_refs(text, |_| "0".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_only_well_formed_references() {
        assert!(is_project_tool_ref("${VELNOR_TOOL_RUST}"));
        assert!(!is_project_tool_ref("${VELNOR_TOOL_}"));
        assert!(!is_project_tool_ref("${VELNOR_TOOL_rust}"));
        assert!(!is_project_tool_ref("x${VELNOR_TOOL_RUST}"));
        let text = "rust@${VELNOR_TOOL_RUST} ${VELNOR_TOOL_MR_BOXINGTON}-a ${VELNOR_TOOL_";
        assert_eq!(
            project_tool_ref_names(text),
            ["VELNOR_TOOL_RUST", "VELNOR_TOOL_MR_BOXINGTON"]
        );
        assert_eq!(
            project_tool_refs_to_expressions(text),
            "rust@${{ env.VELNOR_TOOL_RUST }} ${{ env.VELNOR_TOOL_MR_BOXINGTON }}-a ${VELNOR_TOOL_"
        );
        assert_eq!(without_project_tool_refs(text), "rust@0 0-a ${VELNOR_TOOL_");
        assert!(!has_project_tool_ref("rust@1.2.3"));
    }

    #[test]
    fn identity_input_binds_exactly_the_installed_project_tools() {
        let digest = "a".repeat(64);
        let input = tools_identity_input(&digest, ["VELNOR_TOOL_RUST", "VELNOR_TOOL_NEXTEST"]);
        assert_eq!(
            input,
            format!(
                "{digest}-${{VELNOR_TOOL_NEXTEST}}-${{VELNOR_TOOL_NEXTEST_LOCK}}-${{VELNOR_TOOL_RUST}}-${{VELNOR_TOOL_RUST_LOCK}}"
            )
        );
        assert!(is_tools_identity_input(&input));
        assert!(is_tools_identity_input(&digest));
        assert!(!input.contains("hashFiles"));
        for bad in [
            format!("{digest}-${{VELNOR_TOOL_RUST}}"),
            format!("{digest}-${{VELNOR_TOOL_RUST_LOCK}}"),
            format!("{digest}-${{VELNOR_TOOL_NODE}}-${{VELNOR_TOOL_NODE_LOCK}}"),
            format!("{digest}-${{{{ hashFiles('mise.toml') }}}}"),
            format!(
                "{digest}-${{VELNOR_TOOL_RUST}}-${{VELNOR_TOOL_RUST_LOCK}}-${{VELNOR_TOOL_NEXTEST}}-${{VELNOR_TOOL_NEXTEST_LOCK}}"
            ),
            "A".repeat(64),
            "a".repeat(63),
        ] {
            assert!(!is_tools_identity_input(&bad), "{bad}");
        }
        assert_eq!(
            project_tool_lock_base("VELNOR_TOOL_RUST_LOCK"),
            Some("VELNOR_TOOL_RUST")
        );
        assert_eq!(project_tool_lock_base("VELNOR_TOOL_RUST"), None);
        for (name, _) in crate::config::project_tool_resolver::PROJECT_TOOL_KEYS {
            assert!(!name.ends_with(PROJECT_TOOL_LOCK_SUFFIX), "{name}");
        }
    }
}
