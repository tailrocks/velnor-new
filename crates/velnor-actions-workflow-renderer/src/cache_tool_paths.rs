//! Shared fail-closed ownership checks for tool-cache transport paths.

use velnor_actions_contract::{Step, StepKind};

use velnor_actions_contract::ToolCacheDomain;

pub(crate) fn owned_transport(step: &Step) -> bool {
    [
        ToolCacheDomain::Planning,
        ToolCacheDomain::Full,
        ToolCacheDomain::NpmBootstrap,
        ToolCacheDomain::BunBootstrap,
        ToolCacheDomain::TofuBootstrap,
        ToolCacheDomain::GradleBootstrap,
    ]
    .into_iter()
    .any(|domain| transport_in_domain(step, domain))
}

pub(crate) fn transport_in_domain(step: &Step, domain: ToolCacheDomain) -> bool {
    let StepKind::Action { uses, with, .. } = &step.kind else {
        return false;
    };
    (uses.starts_with("actions/cache/") || uses.starts_with("actions/cache@"))
        && with.get("path").is_some_and(|paths| {
            paths
                .lines()
                .any(|path| domain.payload().iter().any(|root| overlaps(path, root)))
        })
}

/// Known Mise executable spelling; detection never grants installation authority.
pub(crate) fn is_mise_word(word: &str) -> bool {
    word == "mise" || mise_word_domain(word, Some(ToolCacheDomain::Full.root())).is_some()
}

/// Resolve only a known executable spelling in its explicit step environment.
pub(crate) fn mise_word_domain(word: &str, mise_data_dir: Option<&str>) -> Option<ToolCacheDomain> {
    let mut resolved = word.to_owned();
    if mise_data_dir.is_none()
        && (word.starts_with("$MISE_DATA_DIR/")
            || word.starts_with("${MISE_DATA_DIR}/")
            || word.starts_with("%MISE_DATA_DIR%/")
            || word
                .strip_prefix("${{")
                .and_then(|tail| tail.split_once("}}"))
                .is_some_and(|(name, _)| name.trim() == "env.MISE_DATA_DIR"))
    {
        return None;
    }
    if let Some(root) = mise_data_dir {
        for prefix in [
            "$MISE_DATA_DIR",
            "${MISE_DATA_DIR}",
            "${{ env.MISE_DATA_DIR }}",
        ] {
            if let Some(suffix) = word.strip_prefix(prefix)
                && suffix.starts_with('/')
            {
                resolved = format!("{root}{suffix}");
                break;
            }
        }
        if let Some(expression) = word.strip_prefix("${{")
            && let Some((name, suffix)) = expression.split_once("}}")
            && name.trim() == "env.MISE_DATA_DIR"
        {
            resolved = format!("{root}{suffix}");
        }
    }
    [
        ToolCacheDomain::Planning,
        ToolCacheDomain::Full,
        ToolCacheDomain::NpmBootstrap,
        ToolCacheDomain::BunBootstrap,
        ToolCacheDomain::TofuBootstrap,
        ToolCacheDomain::GradleBootstrap,
    ]
    .into_iter()
    .find(|domain| {
        canonical_path(&resolved) == canonical_path(&format!("{}/bin/mise", domain.root()))
    })
}

fn overlaps(path: &str, root: &str) -> bool {
    let path = path.trim();
    if path.starts_with('!') {
        return false;
    }
    let Some(path) = canonical_path(path) else {
        // An unresolved variable may resolve to any owned root.
        return true;
    };
    let Some(root) = canonical_path(root) else {
        return true;
    };
    let prefix = path.split(['*', '?', '[', '{']).next().unwrap_or_default();
    let prefix = prefix.trim_end_matches('/');
    prefix.is_empty()
        || prefix == root
        || prefix.starts_with(&format!("{root}/"))
        || root.starts_with(&format!("{prefix}/"))
        || (prefix != path && root.starts_with(prefix))
}

/// Normalize lexical spelling only; never traverse a restored filesystem.
fn canonical_path(path: &str) -> Option<String> {
    let path = expand_owned_prefix(path)?;
    if path.contains('$') || path.contains('%') {
        return None;
    }
    let path = path.replace('\\', "/");
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.truncate(parts.len().saturating_sub(1));
            }
            _ => parts.push(part),
        }
    }
    Some(parts.join("/"))
}

fn expand_owned_prefix(path: &str) -> Option<String> {
    if let Some(tail) = path.strip_prefix("${{") {
        let (expression, suffix) = tail.split_once("}}")?;
        let root = expression_root(expression.trim())?;
        return Some(format!("{root}{suffix}"));
    }
    for (alias, root) in [
        ("RUNNER_TEMP", "@runner-temp"),
        ("MISE_DATA_DIR", "@runner-temp/velnor/mise"),
        ("RUSTUP_HOME", "@runner-temp/velnor/rustup"),
        ("CARGO_HOME", "@runner-temp/velnor/cargo"),
    ] {
        for prefix in [
            format!("${alias}"),
            format!("${{{alias}}}"),
            format!("%{alias}%"),
        ] {
            if let Some(suffix) = path.strip_prefix(&prefix)
                && (suffix.is_empty() || suffix.starts_with(['/', '\\']))
            {
                return Some(format!("{root}{suffix}"));
            }
        }
    }
    Some(path.to_owned())
}

fn expression_root(expression: &str) -> Option<&'static str> {
    match expression {
        "runner.temp" | "env.RUNNER_TEMP" => Some("@runner-temp"),
        "env.MISE_DATA_DIR" => Some("@runner-temp/velnor/mise"),
        "env.RUSTUP_HOME" => Some("@runner-temp/velnor/rustup"),
        "env.CARGO_HOME" => Some("@runner-temp/velnor/cargo"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::mise_word_domain;
    use velnor_actions_contract::ToolCacheDomain;

    #[test]
    fn mise_data_dir_aliases_use_the_step_domain() {
        let planning = ToolCacheDomain::Planning.root();
        for word in [
            "$MISE_DATA_DIR/bin/mise",
            "${MISE_DATA_DIR}/bin/mise",
            "${{ env.MISE_DATA_DIR }}/bin/mise",
        ] {
            assert_eq!(
                mise_word_domain(word, Some(planning)),
                Some(ToolCacheDomain::Planning),
                "{word}"
            );
        }
        assert_eq!(
            mise_word_domain("$MISE_DATA_DIR/bin/mise", None),
            None,
            "unbound alias must not grant a domain"
        );
    }

    #[test]
    fn explicit_full_mise_path_stays_full_in_planning_context() {
        let full_path = "${{ runner.temp }}/velnor/mise/bin/mise";
        assert_eq!(
            mise_word_domain(full_path, Some(ToolCacheDomain::Planning.root())),
            Some(ToolCacheDomain::Full)
        );
    }
}
