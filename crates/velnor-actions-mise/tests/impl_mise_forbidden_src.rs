//! Source policy: the mise crate must never grow file writes, toolchain
//! managers, installer actions, project-config reads, or tool-file
//! management subcommands. Scans `src/` code lines (`//` comments stripped
//! so docs may discuss the forbidden surface).

use std::path::{Path, PathBuf};

fn src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn expected_modules() -> Vec<&'static str> {
    vec![
        "cache.rs",
        "catalog.rs",
        "command.rs",
        "error.rs",
        "gate6.rs",
        "git.rs",
        "lib.rs",
        "lock.rs",
        "nextest.rs",
        "preflight.rs",
        "requests.rs",
        "template.rs",
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

fn check_file(path: &Path, violations: &mut Vec<String>) -> Result<(), String> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .ok_or_else(|| format!("nameless file: {}", path.display()))?;
    for (line, code) in code_of(path)? {
        let at = format!("{name}:{line}");
        for token in [
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
            "mise.toml",
            "mise.lock",
            "rust-toolchain.toml",
            "mise-version",
            "apply-fix",
            "apply_fix",
        ] {
            if code.contains(token) {
                violations.push(format!("{at}: forbidden `{token}` in `{code}`"));
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
