//! Prepare-path scan split from `impl_orch_f2a` (400-line gate).

use crate::impl_common::TestResult;
use crate::impl_orch_f2a::{code_of, orch_src, product_src_files};

#[test]
fn plan_and_generate_share_one_prepare_path() -> TestResult {
    let mut discover_calls = Vec::new();
    let mut config_calls = Vec::new();
    for path in product_src_files()? {
        let name = path
            .file_name()
            .map(|file| file.to_string_lossy().into_owned())
            .unwrap_or_default();
        if name.ends_with("_tests.rs") {
            continue;
        }
        for (line, code) in code_of(&path)? {
            if code.contains("discover(") && !code.contains("fn discover(") {
                discover_calls.push(format!("{name}:{line}"));
            }
            if code.contains("load_config(") && !code.contains("fn load_config(") {
                config_calls.push(format!("{name}:{line}"));
            }
        }
    }
    assert_eq!(discover_calls.len(), 1, "{discover_calls:?}");
    assert!(
        discover_calls[0].starts_with("prepare.rs"),
        "{discover_calls:?}"
    );
    assert_configuration_admission_boundaries(&config_calls);
    let internal = std::fs::read_to_string(orch_src().join("internal.rs"))?;
    assert!(internal.contains("prepare(&root)"), "plan runs prepare");
    let generate = std::fs::read_to_string(orch_src().join("generate.rs"))?;
    assert!(
        generate.contains("prep: &GenerationPreparation"),
        "generate consumes preparation"
    );
    Ok(())
}

fn assert_configuration_admission_boundaries(config_calls: &[String]) {
    // Preparation discovers repository evidence, execution reloads the
    // source-bound definition, routing reloads before schema migration,
    // the owned-tool preview reloads at its CLI-gated boundary, qualification
    // admission reloads for the repo-resolved catalog, and tap preparation
    // reloads the captured tap's own config before using its updater task.
    // Backend cache operations read source configuration before each request.
    // Preview admission and producer checks load configuration at separate boundaries.
    assert_eq!(config_calls.len(), 9, "{config_calls:?}");
    assert!(
        config_calls
            .iter()
            .any(|call| call.starts_with("prepare.rs")),
        "{config_calls:?}"
    );
    assert!(
        config_calls
            .iter()
            .any(|call| call.starts_with("check_runtime.rs")),
        "{config_calls:?}"
    );
    assert!(
        config_calls
            .iter()
            .any(|call| call.starts_with("routing.rs")),
        "{config_calls:?}"
    );
    assert!(
        config_calls
            .iter()
            .any(|call| call.starts_with("owned_tool_preview.rs")),
        "{config_calls:?}"
    );
    assert!(
        config_calls
            .iter()
            .any(|call| call.starts_with("qualification_resolver.rs")),
        "{config_calls:?}"
    );
    assert!(
        config_calls.iter().all(|call| {
            call.starts_with("prepare.rs")
                || call.starts_with("preview_producer_entry.rs")
                || call.starts_with("check_runtime.rs")
                || call.starts_with("routing.rs")
                || call.starts_with("owned_tool_preview.rs")
                || call.starts_with("qualification_cache_backend.rs")
                || call.starts_with("qualification_resolver.rs")
                || call.starts_with("tap_prepare.rs")
                || call.starts_with("tap_prepare_execute.rs")
        }),
        "{config_calls:?}"
    );
}
