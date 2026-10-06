//! Selected source probes share the producer's native Cargo tree selection.

use std::collections::BTreeMap;

use velnor_actions_contract::Step;
use velnor_actions_mise::{PinnedTool, ToolCatalog};

use crate::OrchestratorError;

use super::descriptor::{RustSourceDescriptor, RustSourceSelection};

// Repository values are positional arguments; only this fixed template is code.
const PROBE_SCRIPT: &str = "cd \"$GITHUB_WORKSPACE\" && root=\"$1\" && shift && \
    if [ \"$root\" = . ]; then root=; fi && \
    if \"$@\" --manifest-path \"$GITHUB_WORKSPACE/${root:+$root/}Cargo.toml\" \
    --offline >/dev/null 2>&1; then \
    echo \"velnor: sources hit, skipping fetch\"; else \
    echo \"velnor: sources miss (source_missing), fetching native_tree_selected_containing\"; \
    \"$@\" --manifest-path \"$GITHUB_WORKSPACE/${root:+$root/}Cargo.toml\"; fi";

/// Complete workspace descriptors keep the existing complete fetch fallback.
pub(crate) fn steps(
    descriptor: &RustSourceDescriptor,
    catalog: &ToolCatalog,
) -> Result<Option<Vec<Step>>, OrchestratorError> {
    let Some(selections) = descriptor.selections() else {
        return Ok(None);
    };
    let env = crate::matrix_step::task_step_env(catalog, &BTreeMap::new(), true)?;
    let mut steps = Vec::with_capacity(selections.len());
    for (index, selection) in selections.iter().enumerate() {
        crate::source_prep::validate_root(&selection.root)?;
        let manifest = crate::discover::workspace_manifest(&selection.root);
        let base = if selection.root.is_empty() {
            crate::source_prep::FETCH_SOURCES_STEP.to_owned()
        } else {
            format!("{} ({manifest})", crate::source_prep::FETCH_SOURCES_STEP)
        };
        let name = format!("{base} (selected {}: {})", index + 1, selection.package);
        steps.push(
            velnor_actions_workflow_renderer::shell_step(
                &name,
                argv(selection, catalog)?,
                env.clone(),
            )
            .map_err(|error| OrchestratorError::Contract {
                problem: error.to_string(),
            })?,
        );
    }
    Ok(Some(steps))
}

fn argv(
    selection: &RustSourceSelection,
    catalog: &ToolCatalog,
) -> Result<Vec<String>, OrchestratorError> {
    // Shell-step contracts reject empty arguments; `.` represents the root.
    let root = if selection.root.is_empty() {
        "."
    } else {
        &selection.root
    };
    let mut argv: Vec<String> = [
        "sh",
        "-c",
        PROBE_SCRIPT,
        "velnor-source",
        root,
        "mise",
        "--no-config",
        "--no-env",
        "--no-hooks",
        "exec",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    argv.extend([
        catalog.tool_spec(PinnedTool::Rust)?,
        "--".to_owned(),
        "cargo".to_owned(),
        "tree".to_owned(),
        "--locked".to_owned(),
        "-p".to_owned(),
        selection.package.clone(),
        "-e".to_owned(),
        "normal,build,dev".to_owned(),
    ]);
    if let Some(target) = &selection.target {
        argv.extend(["--target".to_owned(), target.clone()]);
    }
    if !selection.default_features {
        argv.push("--no-default-features".to_owned());
    }
    if !selection.features.is_empty() {
        argv.extend(["--features".to_owned(), selection.features.join(",")]);
    }
    Ok(argv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_values_remain_positional_and_feature_target_flags_match() {
        let selection = RustSourceSelection {
            root: String::new(),
            package: "package-alpha".to_owned(),
            target: Some("x86_64-unknown-linux-gnu".to_owned()),
            features: vec!["alpha".to_owned()],
            default_features: false,
        };
        let run = argv(&selection, &ToolCatalog::pinned()).expect("selected argv");
        assert_eq!(&run[..2], ["sh", "-c"]);
        assert_eq!(run[2], PROBE_SCRIPT);
        assert_eq!(&run[3..5], ["velnor-source", "."]);
        assert!(run.windows(2).any(|pair| pair == ["-p", "package-alpha"]));
        assert!(run.windows(2).any(|pair| pair == ["--features", "alpha"]));
        assert!(
            run.iter()
                .any(|argument| argument == "--no-default-features")
        );
        assert!(
            run.windows(2)
                .any(|pair| pair == ["--target", "x86_64-unknown-linux-gnu"])
        );
        assert!(!run[2].contains("package-alpha"));
    }

    #[test]
    fn default_host_selection_has_no_feature_or_target_override() {
        let selection = RustSourceSelection {
            root: "nested".to_owned(),
            package: "alpha".to_owned(),
            target: None,
            features: Vec::new(),
            default_features: true,
        };
        let run = argv(&selection, &ToolCatalog::pinned()).expect("selected argv");
        assert_eq!(run[4], "nested");
        assert!(!run.iter().any(|argument| matches!(
            argument.as_str(),
            "--features" | "--target" | "--no-default-features"
        )));
    }
}
