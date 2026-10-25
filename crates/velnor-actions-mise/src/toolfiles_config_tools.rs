use std::collections::BTreeMap;

use crate::toml_scan::{TomlValue, parse_toml};

const NON_VERSION_SELECTORS: &[&str] = &["path", "prefix", "ref"];

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
            Some(_) => {}
        }
    }
    Ok(tools)
}

fn field_is_choice(field: &str) -> bool {
    field == "version" || NON_VERSION_SELECTORS.contains(&field)
}

fn line_problem(line: u32, problem: &str) -> String {
    format!("line {line}: {problem}")
}
