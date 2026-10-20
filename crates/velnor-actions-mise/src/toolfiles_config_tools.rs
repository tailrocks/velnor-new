//! `[tools]` choices of one Mise config file, read structurally.
//!
//! Mise accepts a tool as a scalar version (`rust = "1.98.1"`) or as a
//! tool table whose `version` field is the choice and whose other fields
//! are options (`rust = { version = "1.98.1", os = ["linux"] }`, the
//! `[tools.rust]` header, or the `rust.version` dotted key). Every
//! spelling reaches the same flattened path, so none can hide a pin.
//! Choices this adapter cannot report as one version string fail.

use std::collections::BTreeMap;

use crate::toml_scan::{TomlValue, parse_toml};

/// Tool-table fields that select a tool without a version.
const NON_VERSION_SELECTORS: &[&str] = &["path", "prefix", "ref"];

/// Tool name to declared version string, from one config file's text.
///
/// # Errors
///
/// `line N: problem` for malformed TOML, a `[[tools.<name>]]` array
/// table, a version that is not one string, a tool declared twice, or a
/// non-version selector.
pub(super) fn parse_config_tools(content: &str) -> Result<BTreeMap<String, String>, String> {
    let doc = parse_toml(content).map_err(|failed| line_problem(failed.line, &failed.problem))?;
    if let Some((path, line)) = doc
        .array_sections
        .iter()
        .find(|(path, _)| path.first().map(String::as_str) == Some("tools"))
    {
        return Err(line_problem(
            *line,
            &format!("tool_array_table:{}", path.join(".")),
        ));
    }
    let mut tools = BTreeMap::new();
    for assignment in &doc.assignments {
        let line = assignment.line;
        let (tool, field) = match assignment.path.as_slice() {
            [section, ..] if section != "tools" => continue,
            [_, tool] => (tool, None),
            [_, tool, field, rest @ ..] => {
                if !rest.is_empty() && field_is_choice(field) {
                    return Err(line_problem(line, "tool_selector_not_scalar"));
                }
                (tool, Some(field.as_str()))
            }
            _ => return Err(line_problem(line, "invalid_tool_path")),
        };
        match field {
            None | Some("version") => {
                let TomlValue::Str(version) = &assignment.value else {
                    return Err(line_problem(line, "tool_version_not_string"));
                };
                if tools.insert(tool.clone(), version.clone()).is_some() {
                    return Err(line_problem(
                        line,
                        &format!("duplicate_tool_version:{tool}"),
                    ));
                }
            }
            Some(selector) if NON_VERSION_SELECTORS.contains(&selector) => {
                return Err(line_problem(
                    line,
                    &format!("unsupported_tool_selector:{tool}:{selector}"),
                ));
            }
            // Other tool-table fields are options, not tool choices.
            Some(_) => {}
        }
    }
    Ok(tools)
}

/// Whether a tool-table field selects the tool (version or selector).
fn field_is_choice(field: &str) -> bool {
    field == "version" || NON_VERSION_SELECTORS.contains(&field)
}

/// `line N: problem` detail with the parser's one-based line.
fn line_problem(line: u32, problem: &str) -> String {
    format!("line {line}: {problem}")
}
