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
    "bounded_file_filter.rs",
    "bounded_file_filter_process.rs",
    "cache.rs",
    "cache_sources.rs",
    "cache_transport.rs",
    "cache_trust.rs",
    "catalog.rs",
    "catalog_identity.rs",
    "catalog_mbx.rs",
    "catalog_owned_source.rs",
    "catalog_owned_source_tests.rs",
    "catalog_source_build_bootstrap.rs",
    "catalog_source_build_bootstrap_tests.rs",
    "catalog_versions.rs",
    "check_capabilities.rs",
    "check_capabilities_encoding_tests.rs",
    "check_capabilities_probe.rs",
    "check_command.rs",
    "check_container_observation.rs",
    "check_container_observation_tests.rs",
    "check_deadline.rs",
    "check_discovery.rs",
    "check_execution.rs",
    "check_file_read.rs",
    "check_metadata.rs",
    "check_orbstack_app_observation.rs",
    "check_qualified_tool_names_tests.rs",
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
    "command_bounded_file_filter.rs",
    "command_cancellable.rs",
    "command_env.rs",
    "command_git.rs",
    "command_output.rs",
    "command_release_signer.rs",
    "command_release_signer_tests.rs",
    "command_tofu.rs",
    "custom_run.rs",
    "error.rs",
    "gate6.rs",
    "gh.rs",
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
    let source = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    let compact = source
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(" .", ".");
    let is_filter_process = name == "bounded_file_filter_process.rs";
    let is_filter = name == "bounded_file_filter.rs";
    let write_expression = "output.write_all(&buffer[..count])";
    let metadata_expression = "std::fs::symlink_metadata(program)";
    if is_filter_process && compact.matches(write_expression).count() != 1 {
        violations.push(format!(
            "{name}: expected exactly one bounded output write `{write_expression}`"
        ));
    }
    if is_filter && compact.matches(metadata_expression).count() != 1 {
        violations.push(format!(
            "{name}: expected exactly one read-only program metadata check `{metadata_expression}`"
        ));
    }
    if !is_filter_process && compact.contains(write_expression) {
        violations.push(format!(
            "{name}: bounded output write is outside its approved module"
        ));
    }
    if !is_filter && compact.contains(metadata_expression) {
        violations.push(format!(
            "{name}: program metadata check is outside its approved module"
        ));
    }
    for (line, mut code) in code_of(path)? {
        if is_filter_process && code.trim() == ".write_all(&buffer[..count])" {
            code = code.replace("write_all", "");
        }
        if is_filter && code.contains(metadata_expression) {
            code = code.replace(metadata_expression, "");
        }
        let at = format!("{name}:{line}");
        for token in ALWAYS_BANNED {
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
    Ok(())
}

#[test]
fn bounded_file_filter_exceptions_are_exact_and_local() {
    let allowed_write = "fn copy_from_child() { output\n.write_all(&buffer[..count])?; }";
    let altered_write = "fn copy_from_child() { other\n.write_all(&buffer[..count])?; }";
    let extra_write =
        "fn copy_from_child() { output.write_all(&buffer[..count])?; output.write_all(b\"x\")?; }";
    let allowed_metadata = "fn check() { let m = std::fs::symlink_metadata(program)?; }";
    let extra_metadata =
        "fn check() { std::fs::symlink_metadata(program)?; std::fs::symlink_metadata(other)?; }";
    let create_link = "fn bad() { std::os::unix::fs::symlink(source, target)?; }";

    let mut violations = Vec::new();
    check_source_fixture(
        "bounded_file_filter_process.rs",
        allowed_write,
        &mut violations,
    );
    assert!(violations.is_empty(), "{violations:?}");

    violations.clear();
    check_source_fixture(
        "bounded_file_filter_process.rs",
        altered_write,
        &mut violations,
    );
    assert_ne!(violations, Vec::<String>::new());

    violations.clear();
    check_source_fixture(
        "bounded_file_filter_process.rs",
        extra_write,
        &mut violations,
    );
    assert_ne!(violations, Vec::<String>::new());

    violations.clear();
    check_source_fixture("bounded_file_filter.rs", allowed_metadata, &mut violations);
    assert!(violations.is_empty(), "{violations:?}");

    violations.clear();
    check_source_fixture("bounded_file_filter.rs", extra_metadata, &mut violations);
    assert_ne!(violations, Vec::<String>::new());

    violations.clear();
    check_source_fixture("bounded_file_filter.rs", create_link, &mut violations);
    assert_ne!(violations, Vec::<String>::new());

    violations.clear();
    check_source_fixture(
        "bounded_file_filter_tests.rs",
        allowed_write,
        &mut violations,
    );
    assert_ne!(violations, Vec::<String>::new());

    for mutation in [
        "std::fs::write(path, bytes)",
        "File::create(path)",
        "std::fs::create_dir(path)",
        "std::fs::rename(old, new)",
        "std::fs::remove_file(path)",
        "std::fs::set_permissions(path, permissions)",
        "std::os::unix::fs::hard_link(source, target)",
    ] {
        violations.clear();
        check_source_fixture("bounded_file_filter.rs", mutation, &mut violations);
        assert!(!violations.is_empty(), "mutation passed: {mutation}");
    }
}

fn check_source_fixture(name: &str, source: &str, violations: &mut Vec<String>) {
    let compact = source
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .replace(" .", ".");
    let is_filter_process = name == "bounded_file_filter_process.rs";
    let is_filter = name == "bounded_file_filter.rs";
    let write_expression = "output.write_all(&buffer[..count])";
    let metadata_expression = "std::fs::symlink_metadata(program)";
    if is_filter_process && compact.matches(write_expression).count() != 1 {
        violations.push(format!("{name}: missing exact bounded output expression"));
    }
    if is_filter && compact.matches(metadata_expression).count() != 1 {
        violations.push(format!("{name}: missing exact metadata expression"));
    }
    if !is_filter_process && compact.contains(write_expression) {
        violations.push(format!(
            "{name}: bounded output expression is outside its module"
        ));
    }
    if !is_filter && compact.contains(metadata_expression) {
        violations.push(format!("{name}: metadata expression is outside its module"));
    }
    let mut remaining = compact;
    if is_filter_process {
        remaining = remaining.replacen(write_expression, "", 1);
    }
    if is_filter {
        remaining = remaining.replacen(metadata_expression, "", 1);
    }
    for token in ALWAYS_BANNED {
        if remaining.contains(token) {
            violations.push(format!("{name}: forbidden `{token}`"));
        }
    }
}
