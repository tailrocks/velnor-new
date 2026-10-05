//! Python PBS artifact cache-key and selector cases.

use std::collections::BTreeMap;

use velnor_actions_contract::{Job, JobTimeout};
use velnor_actions_mise::{PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::cache_p08::{
    infer_job_tools, mise_cache_key_for_tools, tools_digest,
};

use super::impl_renderer_fixtures::LABEL;

#[test]
fn python_selector_binds_archive_and_extraction_to_cache_key() {
    let spec = ToolCatalog::pinned().tool_spec(PinnedTool::Python);
    let key = mise_cache_key_for_tools(
        "x86_64-unknown-linux-gnu",
        "2026.9.18",
        std::slice::from_ref(&spec),
    )
    .expect("catalog Python selector is accepted");
    assert!(key.ends_with(&tools_digest(std::slice::from_ref(&spec))));
    for (current, changed) in [
        ("20261001", "20261003"),
        (
            "b373a4a4e4e70fc05f368c9b53d7738bf37637682b650d96c742805d2da26c32",
            "0000000000000000000000000000000000000000000000000000000000000000",
        ),
        ("strip_components=1", "strip_components=2"),
        ("bin_path=bin", "bin_path=python/bin"),
    ] {
        let changed_spec = spec.replace(current, changed);
        assert_ne!(spec, changed_spec, "fixture must contain {current}");
        assert_ne!(
            tools_digest(std::slice::from_ref(&spec)),
            tools_digest(std::slice::from_ref(&changed_spec)),
            "cache identity changes with {current}"
        );
        assert!(
            mise_cache_key_for_tools("x86_64-unknown-linux-gnu", "2026.9.18", &[changed_spec])
                .is_err()
        );
    }
}

#[test]
fn tool_inference_accepts_the_catalog_python_http_selector() {
    let spec = ToolCatalog::pinned().tool_spec(PinnedTool::Python);
    let step = velnor_actions_workflow_renderer::shell_step(
        "Prepare Python",
        vec![
            "mise".to_owned(),
            "--no-config".to_owned(),
            "install".to_owned(),
            spec.clone(),
        ],
        BTreeMap::new(),
    )
    .expect("prepare");
    let job = Job {
        display_name: "Python source suite".to_owned(),
        runs_on: LABEL.to_owned(),
        timeout_minutes: JobTimeout::VALIDATOR,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        environment: None,
        steps: vec![step],
    };
    assert_eq!(infer_job_tools(&job), vec![spec]);
}
