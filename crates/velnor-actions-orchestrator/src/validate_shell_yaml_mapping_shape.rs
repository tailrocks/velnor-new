use std::collections::BTreeSet;

use crate::OrchestratorError;

use super::{Kind, Line, fail, mapping_entry};

pub(super) fn alias_site(
    line: &Line,
    path: &[(usize, String)],
) -> Result<Option<(Kind, String)>, OrchestratorError> {
    let step = line.content.strip_prefix("- ");
    if let Some(value) = step.filter(|value| value.starts_with(['&', '*'])) {
        if !is_steps(path) || line.indent != 6 {
            return Err(fail("step_scope_unsupported"));
        }
        return Ok(Some((Kind::Step, value.to_owned())));
    }
    let Some((key, value)) = mapping_entry(&line.content) else {
        return Ok(None);
    };
    if !value.starts_with(['&', '*']) || is_run(line, path) {
        return Ok(None);
    }
    let job_env = path.len() == 2
        && path[0].0 == 0
        && path[0].1 == "jobs"
        && path[1].0 == 2
        && line.indent == 4
        && key == "env";
    let step_mapping = is_step(path) && line.indent == 8 && matches!(key, "env" | "with");
    if !job_env && !step_mapping {
        // Preserve the existing rejection class outside the new closed sites.
        return Ok(None);
    }
    Ok(Some((Kind::Mapping, value.to_owned())))
}

pub(super) fn is_run(line: &Line, path: &[(usize, String)]) -> bool {
    line.indent == 8
        && is_step(path)
        && mapping_entry(&line.content).is_some_and(|(key, _)| key == "run")
}

fn is_steps(path: &[(usize, String)]) -> bool {
    path.len() == 3
        && path[0].0 == 0
        && path[0].1 == "jobs"
        && path[1].0 == 2
        && path[2].0 == 4
        && path[2].1 == "steps"
}

fn is_step(path: &[(usize, String)]) -> bool {
    path.len() == 4 && is_steps(&path[..3]) && path[3].0 == 6 && path[3].1 == "[]"
}

pub(super) fn update_path(line: &Line, path: &mut Vec<(usize, String)>) {
    if line.content.starts_with("- ") {
        path.push((line.indent, "[]".to_owned()));
    } else if let Some((key, _)) = mapping_entry(&line.content) {
        path.push((line.indent, key.to_owned()));
    }
}

pub(super) fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub(super) fn validate_body(
    kind: Kind,
    body: &[Line],
    base: usize,
) -> Result<(), OrchestratorError> {
    let mut keys = BTreeSet::new();
    let mut first = true;
    for line in body {
        if line.content.is_empty() || line.content.starts_with('#') {
            continue;
        }
        if line.indent < base || (kind == Kind::Mapping && line.indent != base) {
            return Err(fail("mapping_shape_unsupported"));
        }
        let Some((key, value)) = mapping_entry(&line.content) else {
            return Err(fail("mapping_entry_unsupported"));
        };
        if value.starts_with('!') {
            return Err(fail("mapping_value_unsupported"));
        }
        if line.indent != base {
            continue;
        }
        // The emitted grammar uses simple scalar keys. Reject quoted-key
        // aliases and alternate spellings instead of guessing YAML key types.
        if !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            || !keys.insert(key)
        {
            return Err(fail("mapping_key_invalid_or_duplicate"));
        }
        if kind == Kind::Step {
            if first && (key != "name" || value.is_empty()) {
                return Err(fail("step_name_must_be_first"));
            }
            if !matches!(
                key,
                "name"
                    | "id"
                    | "if"
                    | "uses"
                    | "run"
                    | "shell"
                    | "working-directory"
                    | "continue-on-error"
                    | "timeout-minutes"
                    | "env"
                    | "with"
            ) {
                return Err(fail("step_field_unsupported"));
            }
        } else if value.is_empty() || value.starts_with('!') {
            return Err(fail("mapping_value_unsupported"));
        }
        first = false;
    }
    if first {
        return Err(fail("mapping_empty"));
    }
    Ok(())
}

pub(super) fn materialize(
    header: &Line,
    kind: Kind,
    body: &[Line],
) -> Result<Vec<Line>, OrchestratorError> {
    let mut result = Vec::with_capacity(body.len() + 1);
    if kind == Kind::Mapping {
        let key = mapping_entry(&header.content)
            .ok_or_else(|| fail("mapping_entry_unsupported"))?
            .0;
        result.push(Line {
            indent: header.indent,
            content: format!("{key}:"),
            raw_content: format!("{key}:"),
        });
        result.extend(body.iter().map(|line| Line {
            indent: header.indent + 2 + line.indent,
            content: line.content.clone(),
            raw_content: line.raw_content.clone(),
        }));
    } else {
        let mut first = true;
        for line in body {
            if line.content.is_empty() || line.content.starts_with('#') {
                continue;
            }
            result.push(Line {
                indent: if first {
                    header.indent
                } else {
                    header.indent + 2 + line.indent
                },
                content: if first {
                    format!("- {}", line.content)
                } else {
                    line.content.clone()
                },
                raw_content: if first {
                    format!("- {}", line.raw_content)
                } else {
                    line.raw_content.clone()
                },
            });
            first = false;
        }
    }
    Ok(result)
}
