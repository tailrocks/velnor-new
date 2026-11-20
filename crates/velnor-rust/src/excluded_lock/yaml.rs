// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

//! The step text of a YAML workflow file, read by line without a YAML parser.

/// The `run` and `working-directory` of each step of a YAML workflow, read by line. A step
/// starts at a line that begins a list item; a `run: |` block runs to the next line that is
/// less indented than its key.
pub(super) fn yaml_steps(text: &str) -> Vec<(String, Option<String>)> {
    let lines: Vec<&str> = text.lines().collect();
    let starts: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.trim_start().starts_with("- "))
        .map(|(index, _)| index)
        .collect();
    let mut steps = Vec::new();
    for (position, &start) in starts.iter().enumerate() {
        let end = starts.get(position + 1).copied().unwrap_or(lines.len());
        let (mut run, mut cwd) = (None, None);
        let mut index = start;
        while index < end {
            let (key, indent, value) = yaml_key(lines[index]);
            index += 1;
            match key {
                "run" if value.starts_with(['|', '>']) => {
                    let mut block = Vec::new();
                    while index < lines.len()
                        && (lines[index].trim().is_empty()
                            || lines[index].len() - lines[index].trim_start().len() > indent)
                    {
                        block.push(lines[index].trim());
                        index += 1;
                    }
                    run = Some(block.join("\n"));
                }
                "run" => run = Some(unquote(value).to_owned()),
                "working-directory" => cwd = Some(unquote(value).to_owned()),
                _ => {}
            }
        }
        if let Some(run) = run {
            steps.push((run, cwd));
        }
    }
    steps
}

/// The key, its indent (the column of the key), and the text after `key:` of one YAML line.
fn yaml_key(line: &str) -> (&str, usize, &str) {
    let trimmed = line.trim_start();
    let text = trimmed.strip_prefix("- ").unwrap_or(trimmed).trim_start();
    let indent = line.len() - text.len();
    match text.split_once(':') {
        Some((key, value)) if !key.contains(' ') => (key, indent, value.trim()),
        _ => ("", indent, ""),
    }
}

fn unquote(value: &str) -> &str {
    let value = value.trim();
    value
        .strip_prefix(['\'', '"'])
        .and_then(|rest| rest.strip_suffix(['\'', '"']))
        .unwrap_or(value)
}
