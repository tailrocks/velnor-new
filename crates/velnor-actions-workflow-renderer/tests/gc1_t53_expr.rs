// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;

use super::*;

fn run_values(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut found = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start().trim_start_matches("- ");
        let Some(value) = trimmed.strip_prefix("run:") else {
            continue;
        };
        let indent = line.len() - line.trim_start().len();
        let value = value.trim();
        let value =
            value.split_once(' ').map_or(
                value,
                |(first, rest)| {
                    if first.starts_with('&') { rest } else { value }
                },
            );
        if value.starts_with(['|', '>']) {
            let body: Vec<&str> = lines[index + 1..]
                .iter()
                .take_while(|next| {
                    next.trim().is_empty() || next.len() - next.trim_start().len() > indent
                })
                .copied()
                .collect();
            found.push(body.join("\n"));
        } else {
            found.push(value.to_owned());
        }
    }
    found
}

fn workflows() -> Result<Vec<(String, String)>, Box<dyn Error>> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows");
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "yml") {
            found.push((path.display().to_string(), std::fs::read_to_string(&path)?));
        }
    }
    found.push(("rendered suites".to_owned(), render_text()?));
    Ok(found)
}

#[test]
fn gc1_t53_generated_run_scripts_hold_no_expressions() -> Result<(), Box<dyn Error>> {
    let mut checked = 0usize;
    for (label, text) in workflows()? {
        for value in run_values(&text) {
            checked += 1;
            if value.contains("${{") {
                return Err(fail(&format!(
                    "{label} splices an expression into a run script"
                )));
            }
        }
    }
    if checked < 100 {
        return Err(fail("the scan saw almost no run scripts"));
    }
    Ok(())
}
