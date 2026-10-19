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

/// Every repository file a project tool version may come from.
///
/// Cache identities that bind project versions before the resolver runs
/// hash these files, so a version bump (or lock change) invalidates them.
pub const PROJECT_TOOL_SOURCE_FILES: [&str; 6] = [
    "mise.toml",
    ".mise.toml",
    "mise.lock",
    ".mise.lock",
    "rust-toolchain.toml",
    "rust-toolchain",
];

/// `${{ hashFiles(...) }}` over [`PROJECT_TOOL_SOURCE_FILES`].
#[must_use]
pub fn project_tool_sources_hash() -> String {
    let files = PROJECT_TOOL_SOURCE_FILES
        .iter()
        .map(|file| format!("'{file}'"))
        .collect::<Vec<_>>()
        .join(",");
    format!("${{{{ hashFiles({files}) }}}}")
}

/// Valid `d` input: a lower-case SHA-256 static digest, optionally bound to
/// the project tool sources hash (hard decision 5).
#[must_use]
pub fn is_tools_cache_identity_input(digest: &str) -> bool {
    let project_suffix = format!("-{}", project_tool_sources_hash());
    let digest = digest.strip_suffix(&project_suffix).unwrap_or(digest);
    digest.len() == 64
        && digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
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
    if is_project_tool_ref(version) {
        return true;
    }
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
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
}
