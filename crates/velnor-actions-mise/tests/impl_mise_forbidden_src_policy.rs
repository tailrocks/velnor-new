/// Tokens banned in every mise source file (writes, managers, installers,
/// project-config creators, and floating selectors).
const ALWAYS_BANNED: &[&str] = &[
    "fs::write",
    "File::create",
    "create_dir",
    "create_new",
    "OpenOptions",
    "write_all",
    "fs::copy",
    "fs::rename",
    "fs::remove",
    "set_permissions",
    "symlink",
    "hard_link",
    "cargo install",
    "install-action",
    "taiki-e",
    "\"use\"",
    "\"lock\"",
    "\"upgrade\"",
    "mise use",
    "mise lock",
    "mise upgrade",
    "tool-versions",
    ".tool-versions",
    "mise-version",
    "apply-fix",
    "apply_fix",
];

const FILESYSTEM_TOKENS: &[&str] = &[
    "fs::write",
    "File::create",
    "create_dir",
    "create_new",
    "OpenOptions",
    "write_all",
    "fs::copy",
    "fs::rename",
    "fs::remove",
    "set_permissions",
    "symlink",
    "hard_link",
];

/// Tool-file names allowed only in `toolfiles.rs` (read-only routing and
/// inspection): owned Mise files plus foreign Rust files for routing.
const TOOLFILES_ONLY: &[&str] = &[
    "mise.toml",
    "mise.lock",
    "rust-toolchain.toml",
    "Cargo.toml",
    "Cargo.lock",
];

#[path = "impl_mise_forbidden_src_ast.rs"]
mod ast;
pub(crate) use ast::{check_filesystem, check_source_graph, check_test_reverse_edges};

pub(super) fn check_code(name: &str, line: usize, code: &str, violations: &mut Vec<String>) {
    check_lexical(name, line, code, violations, true);
}

pub(super) fn check_source_code(name: &str, line: usize, code: &str, violations: &mut Vec<String>) {
    check_lexical(name, line, code, violations, false);
}

fn check_lexical(
    name: &str,
    line: usize,
    code: &str,
    violations: &mut Vec<String>,
    filesystem: bool,
) {
    let at = format!("{name}:{line}");
    for token in ALWAYS_BANNED {
        if !filesystem && FILESYSTEM_TOKENS.contains(token) {
            continue;
        }
        if code.contains(token) {
            violations.push(format!("{at}: forbidden `{token}` in `{code}`"));
        }
    }
    if name != "toolfiles.rs" {
        for token in TOOLFILES_ONLY {
            if code.contains(token) {
                violations.push(format!("{at}: `{token}` outside toolfiles.rs in `{code}`"));
            }
        }
    }
    if code.contains("\"rustup\"") && !code.contains("FORBIDDEN") {
        violations.push(format!("{at}: unsanctioned `rustup` in `{code}`"));
    }
    if code.contains(".mise/tasks") && !code.contains("contains") {
        violations.push(format!("{at}: unsanctioned `.mise/tasks` in `{code}`"));
    }
    if SCRIPT_AUTHORITY.contains(&name) || PURE_PAYLOAD_BUILDERS.contains(&name) {
        for token in EXECUTION_BANNED {
            if code.contains(token) {
                violations.push(format!("{at}: executable authority `{token}`"));
            }
        }
    }
    if !SCRIPT_AUTHORITY.contains(&name) {
        for marker in MANAGER_MARKERS {
            if code.contains(marker) {
                violations.push(format!(
                    "{at}: manager payload outside typed authority `{marker}`"
                ));
            }
        }
    }
}

const SCRIPT_AUTHORITY: &[&str] = &[
    "catalog_rust_bootstrap.rs",
    "catalog_rust_proxies.rs",
    "catalog_rust_health.rs",
    "catalog_rust_prepare.rs",
    "catalog_rust_prepare_body.rs",
];
pub(super) const PURE_PAYLOAD_BUILDERS: &[&str] = &[
    "catalog_root_rust_candidate.rs",
    "catalog_root_rust_candidate_body.rs",
    "catalog_root_rust_candidate_source.rs",
    "catalog_source_intent_control.rs",
    "root_rust_candidate_root.rs",
    "archive_projection.rs",
    "archive_projection_identity.rs",
    "catalog_admission_context.rs",
    "catalog_cold_foundation.rs",
    "catalog_mbx_action_authority.rs",
    "catalog_mise_acquisition_source_intent.rs",
    "catalog_native_admission_context.rs",
    "catalog_native_control_parts.rs",
    "catalog_native_snapshot_context.rs",
    "catalog_rust_compiler_authority.rs",
    "catalog_rust_compiler_manifest.rs",
    "catalog_rust_prepare_purpose.rs",
    "catalog_source_intent_install.rs",
    "catalog_source_intent_install_body.rs",
    "catalog_source_snapshot_tools.rs",
    "inventory_loader.rs",
    "source_archive_inventory.rs",
    "source_intent_cold_root.rs",
    "catalog_native_root_clear.rs",
    "catalog_rust_prepare_receipt.rs",
    "cache_namespace.rs",
    "catalog_release_prepare_tools.rs",
    "catalog_native_receipt_preparation.rs",
    "catalog_rust_release.rs",
    "catalog_gradle_consumer.rs",
    "catalog_gradle_consumer_prepare.rs",
    "catalog_gradle_consumer_recipe.rs",
    "catalog_native_tool_context.rs",
    "catalog_qualification_gradle_bootstrap.rs",
    "catalog_qualification_gradle_consumer.rs",
    "catalog_qualification_release_plz.rs",
    "catalog_rustup_authority.rs",
    "catalog_rustup_metadata_descriptor.rs",
    "catalog_source_build_bootstrap.rs",
    "catalog_tool_prepare_config.rs",
    "catalog_qualification_go.rs",
    "catalog_qualification_semver.rs",
    "catalog_qualification_audit.rs",
    "catalog_selectors.rs",
    "catalog_native_profiles.rs",
    "cache_snapshot.rs",
    "catalog_rust_cold.rs",
    "catalog_qualification_gh.rs",
    "catalog_delivery_tools.rs",
    "catalog_native_desktop.rs",
    "catalog_native_health.rs",
    "catalog_native_validation.rs",
    "catalog_owned_source.rs",
    "catalog_qualification.rs",
    "catalog_qualification_bun.rs",
    "catalog_qualification_gradle.rs",
    "catalog_qualification_identity.rs",
    "catalog_qualification_install.rs",
    "catalog_qualification_java.rs",
    "catalog_qualification_mbx.rs",
    "catalog_qualification_native.rs",
    "catalog_qualification_node.rs",
    "catalog_qualification_python.rs",
    "catalog_qualification_records.rs",
    "catalog_qualification_tofu.rs",
    "catalog_qualification_types.rs",
    "catalog_qualification_uv.rs",
    "catalog_qualification_validate.rs",
    "catalog_rust_prepare_exec_env.rs",
    "catalog_java_materialize.rs",
    "catalog_mise_acquisition.rs",
    "catalog_tool_prepare.rs",
    "catalog_tool_prepare_body.rs",
];
const EXECUTION_BANNED: &[&str] = &[
    "process::",
    "Command::",
    "IsolatedCommand",
    "PinnedToolExec",
    "std::fs",
    "use std::{fs",
    ".spawn(",
    ".status(",
    ".output(",
    ".run(",
];
const MANAGER_MARKERS: &[&str] = &[
    "$CARGO_HOME/bin/rustup",
    "rustup-init",
    "$RUSTUP_HOME/velnor-integrity",
];

#[test]
fn payload_authority_cannot_execute_or_write_on_generator_host() {
    for module in SCRIPT_AUTHORITY.iter().chain(PURE_PAYLOAD_BUILDERS) {
        for code in ALWAYS_BANNED.iter().chain(EXECUTION_BANNED) {
            let mut violations = Vec::new();
            check_code(module, 1, code, &mut violations);
            assert!(!violations.is_empty(), "accepted {module}: {code}");
        }
    }
}

#[test]
fn nested_sources_and_manager_authority_keep_existing_guards() {
    for (name, code) in [
        ("requests/runtime.rs", "fs::write(path, bytes)"),
        ("catalog_rust_desktop.rs", "\"Cargo.toml\""),
        ("command.rs", "\"$CARGO_HOME/bin/rustup\""),
        ("catalog_java_materialize.rs", "\"$CARGO_HOME/bin/rustup\""),
    ] {
        let mut violations = Vec::new();
        check_code(name, 1, code, &mut violations);
        assert!(!violations.is_empty(), "accepted {name}: {code}");
    }
}

#[test]
fn generated_manager_payload_is_limited_to_named_builders() {
    for module in PURE_PAYLOAD_BUILDERS {
        for marker in MANAGER_MARKERS {
            let mut violations = Vec::new();
            check_code(module, 1, marker, &mut violations);
            assert!(
                !violations.is_empty(),
                "accepted manager payload in {module}"
            );
        }
    }
    for module in SCRIPT_AUTHORITY {
        let mut violations = Vec::new();
        check_code(
            module,
            1,
            "const SCRIPT: &str = \"$CARGO_HOME/bin/rustup --version\";",
            &mut violations,
        );
        assert!(violations.is_empty(), "rejected static payload in {module}");
        check_code(
            module,
            2,
            "FORBIDDEN Command::new(\"bash\")",
            &mut violations,
        );
        assert!(!violations.is_empty(), "FORBIDDEN bypassed execution guard");
    }
}
