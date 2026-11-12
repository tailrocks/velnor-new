// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

fn leading_spaces(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn step_block<'a>(lines: &[&'a str], at: usize) -> Vec<&'a str> {
    let dash = leading_spaces(lines[at]);
    lines[at + 1..]
        .iter()
        .take_while(|line| line.trim().is_empty() || leading_spaces(line) > dash)
        .copied()
        .collect()
}

fn expand_step_aliases(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut blocks = BTreeMap::new();
    for (at, line) in lines.iter().enumerate() {
        if let Some(name) = line.trim().strip_prefix("- &s") {
            blocks.insert(
                name.to_owned(),
                (leading_spaces(line), step_block(&lines, at)),
            );
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while at < lines.len() {
        let line = lines[at];
        let trimmed = line.trim();
        let name = trimmed
            .strip_prefix("- &s")
            .or_else(|| trimmed.strip_prefix("- *s"));
        let Some((origin, block)) = name.and_then(|name| blocks.get(name)) else {
            out.push_str(line);
            out.push('\n');
            at += 1;
            continue;
        };
        let dash = leading_spaces(line);
        for (index, body) in block.iter().enumerate() {
            if index == 0 {
                out.extend([
                    " ".repeat(dash),
                    "- ".to_owned(),
                    body.trim_start().to_owned(),
                ]);
            } else if dash >= *origin {
                out.extend([" ".repeat(dash - origin), (*body).to_owned()]);
            } else {
                out.push_str(&body[(origin - dash).min(body.len())..]);
            }
            out.push('\n');
        }
        at += 1;
        if trimmed.starts_with("- &s") {
            at += block.len();
        }
    }
    out
}

pub(super) fn expand_aliases(text: &str) -> String {
    let text = expand_step_aliases(text);
    let text = text.as_str();
    let mut anchors = BTreeMap::new();
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("run: &")
            && let Some((name, value)) = rest.split_once(' ')
        {
            anchors.insert(name.to_owned(), value.to_owned());
        }
    }
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let alias = line
            .trim()
            .strip_prefix("run: *")
            .and_then(|name| anchors.get(name));
        match alias {
            Some(value) => {
                let indent = &line[..line.len() - line.trim_start().len()];
                out.extend([indent, "run: ", value]);
            }
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}
