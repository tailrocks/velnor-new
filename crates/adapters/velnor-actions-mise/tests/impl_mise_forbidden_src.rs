//! Source policy: the mise crate must never grow file writes, toolchain
//! managers, installer actions, project-config reads, or tool-file
//! management subcommands. Scans `src/` code lines recursively (`//`
//! comments stripped so docs may discuss the forbidden surface).
//! Canonical unit suites (`tests.rs`) are inventoried but exempt from the
//! token scan: fixtures necessarily perform IO (temp dirs, fixture writes).

use std::path::{Path, PathBuf};

fn src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn expected_modules() -> Vec<&'static str> {
    vec![
        "build.rs",
        "cache.rs",
        "cache_sources.rs",
        "cache_transport.rs",
        "cache_trust.rs",
        "catalog.rs",
        "catalog/lock.rs",
        "catalog/lock_verify.rs",
        "catalog/mbx.rs",
        "catalog/release_plz.rs",
        "catalog/versions.rs",
        "check_deadline.rs",
        "check_deadline/tests.rs",
        "check_tool_probes.rs",
        "checks.rs",
        "checks/capabilities.rs",
        "checks/capabilities/observation.rs",
        "checks/capabilities/observation/app.rs",
        "checks/capabilities/observation/tests.rs",
        "checks/capabilities/probe.rs",
        "checks/capabilities/probe/tests.rs",
        "checks/capabilities/tests.rs",
        "checks/discovery.rs",
        "checks/discovery/projection.rs",
        "checks/discovery/qualified.rs",
        "checks/discovery/qualified/tests.rs",
        "checks/discovery/qualified/tests/identity_tests.rs",
        "checks/discovery/qualified/tests/names_tests.rs",
        "checks/discovery/qualified/tests/resolve_tests.rs",
        "checks/discovery/task_validation.rs",
        "checks/execution.rs",
        "checks/file_read.rs",
        "checks/file_read/tests.rs",
        "checks/metadata.rs",
        "checks/system_tools.rs",
        "checks/system_tools/tests.rs",
        "command.rs",
        "command/cancellable.rs",
        "command/check.rs",
        "command/env.rs",
        "command/output.rs",
        "command/qualified_acquisition.rs",
        "command/tofu.rs",
        "custom_run.rs",
        "error.rs",
        "gate6.rs",
        "gh.rs",
        "git.rs",
        "git/tests.rs",
        "lib.rs",
        "nextest/mod.rs",
        "nextest_config/mod.rs",
        "nextest_plan/mod.rs",
        "nextest_shapes/mod.rs",
        "preflight.rs",
        "requests.rs",
        "restore.rs",
        "restore_evidence.rs",
        "reuse.rs",
        "runtime_paths.rs",
        "steps.rs",
        "template.rs",
        "toml_parser.rs",
        "toml_scan.rs",
        "toml_strings.rs",
        "toolfiles.rs",
        "toolfiles/lockfile.rs",
        "verify.rs",
        "wrappers.rs",
    ]
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
    let mut pending = vec![dir.clone()];
    while let Some(current) = pending.pop() {
        let entries = std::fs::read_dir(&current).map_err(|err| err.to_string())?;
        for entry in entries {
            let path = entry.map_err(|err| err.to_string())?.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if !path.extension().is_some_and(|ext| ext == "rs") {
                continue;
            }
            let relative = path
                .strip_prefix(&dir)
                .map_err(|err| err.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            found.push(relative);
            let is_suite = path.file_name().is_some_and(|name| name == "tests.rs");
            if !is_suite {
                check_file(&path, &mut violations)?;
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
    for (line, code) in code_of(path)? {
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
