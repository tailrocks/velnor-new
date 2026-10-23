//! Source policy: the mise crate must never grow file writes, toolchain
//! managers, installer actions, project-config reads, or tool-file
//! management subcommands. Scans `src/` code lines (`//` comments stripped
//! so docs may discuss the forbidden surface).

use std::path::{Path, PathBuf};

fn src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

const EXPECTED_MODULES: &[&str] = &[
    "build.rs",
    "cache.rs",
    "cache_sources.rs",
    "cache_transport.rs",
    "cache_trust.rs",
    "catalog.rs",
    "catalog_identity.rs",
    "catalog_jactionlint_digests.rs",
    "catalog_mbx.rs",
    "catalog_owned_source.rs",
    "catalog_owned_source_tests.rs",
    "catalog_project.rs",
    "catalog_project_tests.rs",
    "catalog_rust_manifests.rs",
    "catalog_source_build_bootstrap.rs",
    "catalog_source_build_bootstrap_tests.rs",
    "catalog_tool.rs",
    "catalog_versions.rs",
    "check_capabilities.rs",
    "check_capabilities_encoding_tests.rs",
    "check_capabilities_probe.rs",
    "check_command.rs",
    "check_container_observation.rs",
    "check_container_observation_tests.rs",
    "check_deadline.rs",
    "check_deadline_scope_tests.rs",
    "check_discovery.rs",
    "check_execution.rs",
    "check_file_read.rs",
    "check_metadata.rs",
    "check_orbstack_app_observation.rs",
    "check_qualified_tool_names_tests.rs",
    "check_qualified_tool_shellcheck_tests.rs",
    "check_qualified_tools.rs",
    "check_qualified_tools_tests.rs",
    "check_system_tools.rs",
    "check_system_tools_parse.rs",
    "check_system_tools_tests.rs",
    "check_task_validation.rs",
    "check_tool_probes.rs",
    "check_tool_projection.rs",
    "checks.rs",
    "command.rs",
    "command_cancellable.rs",
    "command_credential.rs",
    "command_env.rs",
    "command_git.rs",
    "command_output.rs",
    "command_private_fetch.rs",
    "command_progress.rs",
    "command_progress_tests.rs",
    "command_release_signer.rs",
    "command_release_signer_tests.rs",
    "command_stream.rs",
    "command_stream_tests.rs",
    "command_tofu.rs",
    "custom_run.rs",
    "error.rs",
    "gate6.rs",
    "git.rs",
    "git_tap_capture.rs",
    "git_tap_capture_tests.rs",
    "lib.rs",
    "lock.rs",
    "lock_verify.rs",
    "mise_lockfile.rs",
    "nextest.rs",
    "nextest_config.rs",
    "nextest_plan.rs",
    "nextest_shapes.rs",
    "preflight.rs",
    "package_release_tools.rs",
    "package_release_tools_tests.rs",
    "qualified_acquisition.rs",
    "qualified_acquisition_tests.rs",
    "release_plz.rs",
    "requests.rs",
    "requests_app_token_tests.rs",
    "requests_helpers.rs",
    "requests_metadata.rs",
    "requests_presence.rs",
    "requests_presence_tests.rs",
    "restore.rs",
    "restore_evidence.rs",
    "reuse.rs",
    "runtime_paths.rs",
    "steps.rs",
    "steps_rust_target.rs",
    "template.rs",
    "toml_parser.rs",
    "toml_scan.rs",
    "toml_strings.rs",
    "toolfiles.rs",
    "toolfiles_config_tools.rs",
    "verify.rs",
    "wrappers.rs",
];

fn expected_modules() -> Vec<&'static str> {
    EXPECTED_MODULES.to_vec()
}

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

fn code_of(path: &Path) -> Result<Vec<(usize, String)>, String> {
    let text = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    Ok(text
        .lines()
        .enumerate()
        .map(|(number, line)| (number + 1, strip_line_comment(line).to_owned()))
        .filter(|(_, line)| !line.trim().is_empty())
        .collect())
}

#[test]
fn mise_sources_stay_read_only_and_unmanaged() -> Result<(), String> {
    let dir = src_dir();
    let mut found: Vec<String> = Vec::new();
    let mut violations: Vec<String> = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|err| err.to_string())?;
    for entry in entries {
        let path = entry.map_err(|err| err.to_string())?.path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .ok_or_else(|| format!("nameless entry: {}", path.display()))?;
            found.push(name);
            check_file(&path, &mut violations)?;
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

/// Tool-file names allowed only in `toolfiles.rs` (read-only routing and
/// inspection): owned Mise files plus foreign Rust files for routing.
const TOOLFILES_ONLY: &[&str] = &[
    "mise.toml",
    "mise.lock",
    "rust-toolchain.toml",
    "Cargo.toml",
    "Cargo.lock",
];

fn check_file(path: &Path, violations: &mut Vec<String>) -> Result<(), String> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .ok_or_else(|| format!("nameless file: {}", path.display()))?;
    let mut borrowed_file_writes = 0;
    let mut diagnostic_writes = 0;
    for (line, code) in code_of(path)? {
        let at = format!("{name}:{line}");
        for token in ALWAYS_BANNED {
            // The stream sink borrows a caller-owned regular File; it cannot create paths.
            if name == "command_stream.rs"
                && *token == "write_all"
                && code.trim() == ".write_all(bytes)"
            {
                borrowed_file_writes += 1;
                continue;
            }
            // Diagnostics borrow a responsive sink, never a filesystem path.
            if name == "command_progress.rs"
                && *token == "write_all"
                && code.trim() == ".write_all(bytes)"
            {
                diagnostic_writes += 1;
                continue;
            }
            // Only this cfg(test) module constructs owned temporary stream fixtures.
            if name == "command_stream_tests.rs" && matches!(*token, "create_new" | "fs::write") {
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
    }
    if name == "command_stream.rs" && borrowed_file_writes != 1 {
        violations.push(format!("{name}: expected exactly one borrowed-file write"));
    }
    if name == "command_progress.rs" && diagnostic_writes != 1 {
        violations.push(format!(
            "{name}: expected exactly one borrowed-diagnostic write"
        ));
    }
    Ok(())
}

#[test]
fn diagnostic_write_admission_is_exactly_one_borrowed_sink_expression() -> Result<(), String> {
    let root = crate::test_temp_dir::TestTempDir::new("mise-diagnostic-policy")
        .map_err(|error| error.to_string())?;
    for (name, source, admitted) in [
        ("command_progress.rs", ".write_all(bytes)\n", true),
        ("command_progress.rs", "", false),
        (
            "command_progress.rs",
            ".write_all(bytes)\n.write_all(bytes)\n",
            false,
        ),
        ("command_progress.rs", ".write_all(other)\n", false),
        ("different.rs", ".write_all(bytes)\n", false),
        (
            "command_progress.rs",
            ".write_all(bytes)\nstd::fs::write(path, bytes)\n",
            false,
        ),
    ] {
        let path = root.path().join(name);
        std::fs::write(&path, source).map_err(|error| error.to_string())?;
        let mut violations = Vec::new();
        check_file(&path, &mut violations)?;
        assert_eq!(
            violations.is_empty(),
            admitted,
            "{name}: {source:?}: {violations:?}"
        );
    }
    Ok(())
}
