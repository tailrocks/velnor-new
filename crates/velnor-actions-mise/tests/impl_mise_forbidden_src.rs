//! Scan every `src/**/*.rs` code line: no writes, direct host managers,
//! installer actions, project-config reads or tool-file management.
//! Fixed payload builders generate scripts without host execution.

use std::path::{Path, PathBuf};

#[path = "impl_mise_forbidden_src_inventory.rs"]
mod inventory;
#[path = "impl_mise_forbidden_src_policy.rs"]
mod policy;
use inventory::source_files;
use policy::{
    PURE_PAYLOAD_BUILDERS, check_filesystem, check_source_code, check_source_graph,
    check_test_reverse_edges,
};

fn src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn expected_modules() -> Vec<&'static str> {
    EXPECTED_MODULES
        .iter()
        .chain(PURE_PAYLOAD_BUILDERS)
        .copied()
        .collect()
}

const EXPECTED_MODULES: &[&str] = &[
    "catalog_root_rust_candidate_source_tests.rs",
    "catalog_root_rust_candidate_tests.rs",
    "root_rust_candidate_root_tests.rs",
    "archive_projection_tests.rs",
    "catalog_cold_foundation_tests.rs",
    "catalog_mbx_action_authority_tests.rs",
    "catalog_qualification_gh_tests.rs",
    "catalog_rust_compiler_authority_tests.rs",
    "source_intent_cold_root_tests.rs",
    "cache_namespace_tests.rs",
    "catalog_gradle_consumer_tests.rs",
    "catalog_native_tool_context_tests.rs",
    "catalog_rustup_authority_tests.rs",
    "catalog_source_build_bootstrap_tests.rs",
    "steps_host.rs",
    "requests/host.rs",
    "catalog_qualification_go_tests.rs",
    "catalog_preparation.rs",
    "catalog_qualification_native_tests.rs",
    "catalog_native_validation_tests.rs",
    "catalog_owned_source_tests.rs",
    "catalog_homebrew.rs",
    "catalog_java_materialize_tests.rs",
    "catalog_tool_prepare_tests.rs",
    "catalog_gradle.rs",
    "catalog_pins.rs",
    "catalog_qualification_tests.rs",
    "catalog_rust_bootstrap.rs",
    "catalog_rust_desktop.rs",
    "catalog_rust_health.rs",
    "catalog_rust_options.rs",
    "catalog_rust_prepare.rs",
    "catalog_rust_prepare_body.rs",
    "catalog_rust_prepare_tests.rs",
    "catalog_rust_proxies.rs",
    "catalog_tools.rs",
    "catalog_workloads.rs",
    "command_workload.rs",
    "command_runtime.rs",
    "workload.rs",
    "workload_java_env.rs",
    "workload_java_env_tests.rs",
    "requests/qualification.rs",
    "requests/runtime.rs",
    "build.rs",
    "cache.rs",
    "cache_sources.rs",
    "cache_transport.rs",
    "cache_trust.rs",
    "catalog.rs",
    "catalog_mbx.rs",
    "catalog_versions.rs",
    "command.rs",
    "command_env.rs",
    "command_git.rs",
    "command_git_diff.rs",
    "command_git_index.rs",
    "command_git_index_checksum.rs",
    "command_git_index_config.rs",
    "command_git_index_format.rs",
    "command_git_index_format_path.rs",
    "command_git_index_format_tests.rs",
    "command_git_index_fs.rs",
    "command_git_index_repository_format.rs",
    "command_git_native.rs",
    "command_git_objects.rs",
    "command_git_owned_config.rs",
    "command_git_owned_config_protocol.rs",
    "command_git_owned_context.rs",
    "command_git_owned_context_source.rs",
    "command_git_owned_context_support.rs",
    "command_git_owned_tests.rs",
    "command_git_private_root.rs",
    "command_git_private_root_files.rs",
    "command_git_private_root_tests.rs",
    "command_git_read.rs",
    "command_git_refs.rs",
    "command_git_repository.rs",
    "command_git_source.rs",
    "command_process.rs",
    "command_output.rs",
    "command_tofu.rs",
    "custom_run.rs",
    "error.rs",
    "gate6.rs",
    "gh.rs",
    "git.rs",
    "lib.rs",
    "lock.rs",
    "lock_verify.rs",
    "mise_lockfile.rs",
    "nextest.rs",
    "nextest_config.rs",
    "nextest_plan.rs",
    "nextest_shapes.rs",
    "preflight.rs",
    "release_plz.rs",
    "requests.rs",
    "restore.rs",
    "restore_evidence.rs",
    "reuse.rs",
    "runtime_paths.rs",
    "steps.rs",
    "steps_homes.rs",
    "template.rs",
    "toml_parser.rs",
    "toml_scan.rs",
    "toml_strings.rs",
    "toolfiles.rs",
    "tool_homes_domain_tests.rs",
    "verify.rs",
    "wrappers.rs",
];

/// Strip a trailing `//` comment, ignoring `//` inside string literals.
fn strip_line_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut quoted = false;
    let mut escape = false;
    let mut index = 0;
    while index + 1 < bytes.len() {
        let byte = bytes[index];
        if escape {
            escape = false;
        } else if byte == b'\\' && quoted {
            escape = true;
        } else if byte == b'"' {
            quoted = !quoted;
        } else if byte == b'/' && bytes[index + 1] == b'/' && !quoted {
            return line[..index].trim_end();
        }
        index += 1;
    }
    line
}

fn code_of(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .map(|(number, line)| (number + 1, strip_line_comment(line).to_owned()))
        .filter(|(_, line)| !line.trim().is_empty())
        .collect()
}

#[test]
fn mise_sources_stay_read_only_and_unmanaged() -> Result<(), String> {
    let dir = src_dir();
    let mut found: Vec<String> = Vec::new();
    let mut violations: Vec<String> = Vec::new();
    let units = inventory::source_units(&dir)?;
    let reverse = inventory::reverse_units(&dir)?;
    check_test_reverse_edges(&units, &reverse, &mut violations);
    check_source_graph(&units, &mut violations);
    const PREFIX: &str = "crates/velnor-actions-mise/src/";
    for (relative, source) in &units {
        if let Some(name) = relative.strip_prefix(PREFIX) {
            found.push(name.to_owned());
            for (line, code) in code_of(source) {
                check_source_code(name, line, &code, &mut violations);
            }
        }
    }
    found.sort_unstable();
    let mut expected = expected_modules();
    expected.sort_unstable();
    assert_eq!(found, expected, "unexpected mise source files");
    assert!(
        violations.is_empty(),
        "forbidden tokens in mise sources:\n{}",
        violations.join("\n")
    );
    Ok(())
}

fn check_file(path: &Path, name: &str, violations: &mut Vec<String>) -> Result<(), String> {
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    check_filesystem(name, &source, violations);
    for (line, code) in code_of(&source) {
        check_source_code(name, line, &code, violations);
    }
    Ok(())
}

#[test]
fn source_discovery_checks_nested_modules_and_unknown_inventory() -> Result<(), String> {
    let root = std::env::temp_dir().join(format!("velnor-source-guard-{}", std::process::id()));
    std::fs::create_dir_all(root.join("requests")).map_err(|error| error.to_string())?;
    std::fs::write(root.join("root.rs"), "// harmless").map_err(|error| error.to_string())?;
    std::fs::write(root.join("requests/runtime.rs"), "fs::write(path, bytes)")
        .map_err(|error| error.to_string())?;
    let files = source_files(&root)?;
    let mut names: Vec<String> = files
        .iter()
        .map(|path| {
            path.strip_prefix(&root)
                .map(|name| name.to_string_lossy().into_owned())
                .map_err(|error| error.to_string())
        })
        .collect::<Result<_, _>>()?;
    names.sort_unstable();
    assert_eq!(names, ["requests/runtime.rs", "root.rs"]);
    let mut violations = Vec::new();
    check_file(
        &root.join("requests/runtime.rs"),
        "requests/runtime.rs",
        &mut violations,
    )?;
    assert!(!violations.is_empty());
    std::fs::write(root.join("requests/unexpected.rs"), "// unexpected")
        .map_err(|error| error.to_string())?;
    assert_eq!(source_files(&root)?.len(), names.len() + 1);
    std::fs::remove_dir_all(root).map_err(|error| error.to_string())?;
    Ok(())
}
