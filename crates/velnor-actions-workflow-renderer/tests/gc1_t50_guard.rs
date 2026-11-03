// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

use velnor_actions_contract::workflow::{
    QUALIFICATION_CACHE_GUARD_ERROR, QUALIFICATION_CACHE_GUARD_ID, QUALIFICATION_CACHE_MODES,
    QUALIFICATION_CACHE_WRITER_IDS, QUALIFICATION_DEFAULT_BRANCH_GATE,
};

use super::*;

fn job_lines<'a>(text: &'a str, id: &str) -> Result<Vec<&'a str>, Box<dyn Error>> {
    let head = format!("  {id}:");
    let mut lines = text.lines().skip_while(|line| *line != head).peekable();
    lines
        .peek()
        .ok_or_else(|| fail(&format!("render lost job:{id}")))?;
    Ok(lines
        .enumerate()
        .take_while(|(at, line)| *at == 0 || line.starts_with("   "))
        .map(|(_, line)| line)
        .collect())
}

fn field<'a>(lines: &[&'a str], indent: usize, key: &str) -> Result<&'a str, Box<dyn Error>> {
    let prefix = format!("{}{key}", " ".repeat(indent));
    lines
        .iter()
        .find_map(|line| line.strip_prefix(prefix.as_str()))
        .ok_or_else(|| fail(&format!("job lost field:{key}")))
}

#[test]
fn qualification_cache_modes_fail_on_the_default_branch_through_a_named_guard()
-> Result<(), Box<dyn Error>> {
    let files = render_schema2_workflows(&request()?)?;
    let text = find_file(&files, QUALIFICATION_WORKFLOW)?;
    let guard = job_lines(text, QUALIFICATION_CACHE_GUARD_ID)?;
    let condition = field(&guard, 4, "if: ")?;
    require(
        condition,
        QUALIFICATION_DEFAULT_BRANCH_GATE,
        "guard condition",
    )?;
    for mode in QUALIFICATION_CACHE_MODES {
        require(
            condition,
            &format!("inputs.mode == '{mode}'"),
            "guard condition",
        )?;
    }
    forbid(&guard.join("\n"), "needs:", "guard")?;
    for id in QUALIFICATION_CACHE_WRITER_IDS {
        let lines = job_lines(text, id)?;
        let writer = field(&lines, 4, "if: ")?;
        let mode = writer
            .split("inputs.mode == '")
            .nth(1)
            .and_then(|rest| rest.split('\'').next())
            .ok_or_else(|| fail(&format!("writer lost its mode:{id}")))?;
        if !QUALIFICATION_CACHE_MODES.contains(&mode) {
            return Err(fail(&format!("guard leaves writer mode unguarded:{mode}")));
        }
    }
    let output = Command::new("sh")
        .arg("-c")
        .arg(field(&guard, 8, "run: ")?)
        .output()?;
    if output.status.code() != Some(1) {
        return Err(fail("guard did not fail the run"));
    }
    require(
        &String::from_utf8_lossy(&output.stdout),
        QUALIFICATION_CACHE_GUARD_ERROR,
        "guard output",
    )
}
