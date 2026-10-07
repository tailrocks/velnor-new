//! Prepare-path scan split from `impl_orch_f2a` (400-line gate).

use crate::impl_common::TestResult;
use crate::impl_orch_f2a::{code_of, family_file, src_files};

#[test]
fn plan_and_generate_share_one_prepare_path() -> TestResult {
    let mut discover_calls = Vec::new();
    let mut config_calls = Vec::new();
    for path in src_files()? {
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
    // Preparation discovers repository evidence, execution reloads the
    // source-bound definition, and routing reloads before schema migration.
    assert_eq!(config_calls.len(), 3, "{config_calls:?}");
    assert!(
        config_calls
            .iter()
            .any(|call| call.starts_with("prepare.rs")),
        "{config_calls:?}"
    );
    assert!(
        config_calls
            .iter()
            .any(|call| call.starts_with("execute.rs")),
        "{config_calls:?}"
    );
    assert!(
        config_calls
            .iter()
            .any(|call| call.starts_with("routing.rs")),
        "{config_calls:?}"
    );
    assert!(
        config_calls.iter().all(|call| {
            call.starts_with("prepare.rs")
                || call.starts_with("execute.rs")
                || call.starts_with("routing.rs")
        }),
        "{config_calls:?}"
    );
    let internal = std::fs::read_to_string(family_file("internal.rs")?)?;
    assert!(internal.contains("prepare(&root)"), "plan runs prepare");
    let generate = std::fs::read_to_string(family_file("generate.rs")?)?;
    assert!(
        generate.contains("prep: &GenerationPreparation"),
        "generate consumes preparation"
    );
    Ok(())
}
