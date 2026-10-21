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
    // reloads the captured tap's own config before using its updater task;
    // preview admission passes its one loaded config to shared preparation;
    // `deps lock` and the private fetch reload it to resolve and verify the declared private sources;
    // `playwright lock` reloads it to resolve the declared browser archives;
    // `unmanaged` reads only the `[github]` and preserve policy, with no discovery.
    assert_eq!(config_calls.len(), 12, "{config_calls:?}");
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
                || call.starts_with("deps_lock.rs")
                || call.starts_with("generate_unmanaged.rs")
                || call.starts_with("private_fetch.rs")
                || call.starts_with("owned_tool_preview.rs")
                || call.starts_with("playwright_resolve.rs")
                || call.starts_with("qualification_resolver.rs")
                || call.starts_with("tap_prepare.rs")
                || call.starts_with("tap_prepare_execute.rs")
        }),
        "{config_calls:?}"
    );
}
