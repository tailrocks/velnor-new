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

#[test]
fn preview_producer_skips_shared_metadata_prepare_after_source_admission() -> TestResult {
    let source = std::fs::read_to_string(orch_src().join("preview_producer_entry.rs"))?;
    let compact_source = source.split_whitespace().collect::<String>();
    let (_, after_preview_branch) = compact_source
        .split_once("PackageReleaseChannel::Preview=>{")
        .ok_or("preview producer channel branch missing")?;
    let branch_tail = after_preview_branch
        .split_once("PackageReleaseChannel::Stable=>{")
        .map(|(preview, _)| preview)
        .ok_or("stable branch must remain an explicit sibling")?;
    let preview_branch =
        &compact_source[compact_source.len() - after_preview_branch.len()..][..branch_tail.len()];
    assert!(
        preview_branch.contains("preflight_preview_source("),
        "Preview validates the admitted A source"
    );
    assert!(
        preview_branch.contains("execution::execute(") && preview_branch.contains("&workspace"),
        "Preview executes directly from the admitted A checkout"
    );
    assert!(
        !preview_branch.contains("prepare_with_config(")
            && !preview_branch.contains("GenerationPreparation"),
        "Preview must not derive its execution inputs through B-sensitive shared preparation"
    );
    let execution = std::fs::read_to_string(orch_src().join("preview_production_execution.rs"))?;
    assert!(
        !execution.contains("GenerationPreparation")
            && !execution.contains("discover(")
            && !execution.contains("cargo metadata"),
        "execution consumes the admitted source root and qualified tools without repository discovery"
    );
    let stable_start = compact_source
        .find("PackageReleaseChannel::Stable=>{")
        .ok_or("stable channel branch missing")?;
    assert!(
        compact_source[stable_start..].contains("prepare_with_config(&workspace,config)"),
        "Stable retains shared preparation"
    );
    Ok(())
}

fn assert_configuration_admission_boundaries(config_calls: &[String]) {
    // Preparation discovers repository evidence, native checks reload the
    // source config to enforce their protected inventory, execution reloads the
    // source-bound definition, routing reloads before schema migration,
    // owned-tool preview and qualification admission reload at their policy
    // boundaries, and tap routing/execution reload the captured tap config.
    // Preview finalization reloads A's config; producer execution validates
    // that config without shared preparation. Private-source, image preview,
    // cache-backend, Playwright, and unmanaged checks each load only what their
    // own admission boundary requires.
    assert_eq!(config_calls.len(), 16, "{config_calls:?}");
    for expected in [
        "prepare.rs",
        "check_runtime.rs",
        "routing.rs",
        "owned_tool_preview.rs",
        "qualification_resolver.rs",
        "preview_source_admission_finalize.rs",
        "native_homebrew_check_required.rs",
        "tap_prepare_routes.rs",
    ] {
        assert!(
            config_calls.iter().any(|call| call.starts_with(expected)),
            "{config_calls:?}"
        );
    }
    assert!(
        config_calls.iter().all(|call| {
            [
                "prepare.rs",
                "preview_producer_entry.rs",
                "preview_source_admission_finalize.rs",
                "check_runtime.rs",
                "native_homebrew_check_required.rs",
                "routing.rs",
                "deps_lock.rs",
                "job_image_preview.rs",
                "generate_unmanaged.rs",
                "private_fetch.rs",
                "owned_tool_preview.rs",
                "qualification_cache_backend.rs",
                "playwright_resolve.rs",
                "qualification_resolver.rs",
                "tap_prepare_execute.rs",
                "tap_prepare_routes.rs",
            ]
            .iter()
            .any(|prefix| call.starts_with(prefix))
        }),
        "{config_calls:?}"
    );
}
