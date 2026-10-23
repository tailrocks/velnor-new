//! Trust-sensitive recognition and construction for project-tool verifiers.

use std::collections::BTreeSet;

use velnor_actions_contract::config::project_tool_resolver::project_tool_resolver_script;
use velnor_actions_contract::workflow::RUST_VERIFIER_USES;

use crate::{RenderError, yaml::Yaml};

use super::{invalid, names, rewrite_mappings, scalar};

pub(super) fn bind_verifier_composite(
    document: &mut Yaml,
    expected_reference: &str,
) -> Result<(), RenderError> {
    let expected = BTreeSet::from([expected_reference.to_owned()]);
    let found = names(document);
    // A project-declared verifier version has exactly one reference and is
    // rewritten to the caller's env binding. A pinned catalog has no project
    // references at all; its exact literal body is checked by the orchestrator
    // audit. Any partial or unrelated reference remains invalid.
    if found.is_empty() {
        return Ok(());
    }
    if found != expected {
        return Err(invalid("verifier_composite_tool_binding"));
    }
    rewrite_mappings(document, false)
}

pub(super) fn verifier_action_ref(step: &Yaml) -> Option<&'static str> {
    let (Yaml::Map(fields)
    | Yaml::AnchoredMap {
        entries: fields, ..
    }) = step
    else {
        return None;
    };
    fields
        .iter()
        .find_map(|(key, value)| (key == "uses").then(|| scalar(value)))
        .flatten()
        .and_then(verifier_action_ref_name)
}

fn verifier_action_ref_name(uses: &str) -> Option<&'static str> {
    if RUST_VERIFIER_USES.contains(&uses) {
        Some("VELNOR_TOOL_RUST")
    } else if uses == crate::verifier_composite::VERIFY_JACTIONLINT_USES {
        Some("VELNOR_TOOL_JACTIONLINT")
    } else {
        None
    }
}

pub(super) fn trusted_resolver_script(
    referenced: &BTreeSet<String>,
    export_rustup: bool,
) -> Result<String, RenderError> {
    const ORIGINAL: &str = r#"velnor_mise() { mise --no-config --no-env --no-hooks "$@"; }"#;
    const TRUSTED: &str = r#"velnor_mise() { mise_bin="${HOME:?missing HOME}/.local/share/mise/bin/mise"; [ -f "$mise_bin" ] && [ -x "$mise_bin" ] && [ ! -L "$mise_bin" ] || velnor_fail "trusted Mise executable missing: $mise_bin"; MISE_DATA_DIR="${HOME:?missing HOME}/.local/share/mise" "$mise_bin" --no-config --no-env --no-hooks "$@"; }"#;
    let mut script = project_tool_resolver_script(referenced, export_rustup);
    if script.matches(ORIGINAL).count() != 1 {
        return Err(invalid("verifier_resolver_mise_binding_shape"));
    }
    script = script.replace(ORIGINAL, TRUSTED);
    Ok(format!(
        "{} PATH={}; export PATH; {script}",
        crate::toolchain_env::credential_unset_prelude(),
        crate::VERIFY_PATH
    ))
}

pub(super) fn contains_verifier_step(node: &Yaml) -> bool {
    match node {
        Yaml::Seq(items) => items.iter().any(contains_verifier_step),
        Yaml::Map(fields)
        | Yaml::AnchoredMap {
            entries: fields, ..
        } => {
            let is_verifier = fields.iter().any(|(key, value)| {
                key == "name"
                    && scalar(value).is_some_and(|name| {
                        matches!(
                            name,
                            "Verify restored Rust toolchain"
                                | crate::VERIFY_RUST_TOOLCHAIN_STEP
                                | "Verify jactionlint"
                        )
                    })
            }) || fields.iter().any(|(key, value)| {
                key == "uses"
                    && scalar(value).is_some_and(|uses| verifier_action_ref_name(uses).is_some())
            });
            is_verifier
                || fields
                    .iter()
                    .any(|(_, value)| contains_verifier_step(value))
        }
        _ => false,
    }
}

pub(super) fn has_trusted_environment_override(node: &Yaml) -> bool {
    let (Yaml::Map(fields)
    | Yaml::AnchoredMap {
        entries: fields, ..
    }) = node
    else {
        return false;
    };
    fields.iter().any(|(key, _)| {
        matches!(
            key.as_str(),
            "HOME" | "MISE_DATA_DIR" | "XDG_DATA_HOME" | "PATH" | "BASH_ENV" | "ENV"
        )
    })
}

pub(super) fn has_verifier_job_environment_override(job: &Yaml) -> bool {
    let (Yaml::Map(fields)
    | Yaml::AnchoredMap {
        entries: fields, ..
    }) = job
    else {
        return false;
    };
    fields
        .iter()
        .any(|(key, value)| key == "env" && has_trusted_environment_override(value))
}
