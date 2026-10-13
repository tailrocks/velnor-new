//! Bounded readonly probe identity admission; output is data, never shell code.
use crate::config::{
    CheckPlatform, MAX_CHECK_QUALIFIED_PROBE_EXPECTED_BYTES, QualifiedTool,
    QualifiedToolExecutable, QualifiedToolProbe,
};
use crate::errors::ContractError;

fn exact_version_token(output: &str, version: &str) -> bool {
    output.match_indices(version).any(|(index, _)| {
        let end = index + version.len();
        let part =
            |byte: u8| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+');
        (index == 0 || !part(output.as_bytes()[index - 1]))
            && (end == output.len() || !part(output.as_bytes()[end]))
    })
}

pub(super) fn validate_probe(
    tool: &QualifiedTool,
    platform: CheckPlatform,
    executable: &QualifiedToolExecutable,
    file: &str,
    key: &str,
) -> Result<(), ContractError> {
    let bad = || ContractError::config(file, key, "invalid_qualified_tool_probe");
    let (expected, multiline) = match &executable.probe {
        QualifiedToolProbe::Version { expected }
        | QualifiedToolProbe::VersionSubcommand { expected } => (expected, false),
        QualifiedToolProbe::CargoNextestVersion { expected }
            if executable.name == "cargo-nextest" =>
        {
            (expected, false)
        }
        QualifiedToolProbe::RustcVerbose { expected } if executable.name == "rustc" => {
            (expected, true)
        }
        _ => return Err(bad()),
    };
    if expected.is_empty()
        || expected.len() > MAX_CHECK_QUALIFIED_PROBE_EXPECTED_BYTES
        || expected.ends_with('\n')
        || !expected
            .bytes()
            .all(|byte| byte.is_ascii_graphic() || byte == b' ' || (multiline && byte == b'\n'))
        || !exact_version_token(expected, &tool.version)
    {
        return Err(bad());
    }
    if multiline {
        let lines: Vec<_> = expected.lines().collect();
        if lines
            .iter()
            .filter(|line| line.starts_with("release: "))
            .count()
            != 1
            || lines
                .iter()
                .filter(|line| line.starts_with("host: "))
                .count()
                != 1
            || !lines.contains(&format!("release: {}", tool.version).as_str())
            || !lines.contains(&format!("host: {}", platform.target()).as_str())
            || !lines.iter().any(|line| {
                line.strip_prefix("commit-hash: ").is_some_and(|value| {
                    value.len() == 40
                        && value
                            .bytes()
                            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                })
            })
        {
            return Err(bad());
        }
    }
    Ok(())
}
