// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0


use crate::RenderError;

pub fn check_no_bare_cargo(yaml: &str) -> Result<(), RenderError> {
    let mut in_run_block = false;
    let mut run_indent = 0;
    for line in yaml.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if in_run_block {
            if trimmed.is_empty() || indent <= run_indent {
                in_run_block = false;
            } else {
                check_command_line(trimmed)?;
                continue;
            }
        }
        if let Some(rest) = trimmed.strip_prefix("run:") {
            let rest = rest.trim();
            if rest == "|" || rest == ">" {
                in_run_block = true;
                run_indent = indent;
            } else if !rest.is_empty() {
                check_command_line(rest)?;
            }
        }
    }
    Ok(())
}

fn check_command_line(command: &str) -> Result<(), RenderError> {
    if command.contains("mise") {
        return Ok(());
    }
    for program in ["cargo", "rustc", "rustup", "mbx", "nextest"] {
        if command.contains(program) {
            return Err(RenderError::BadCommand(format!(
                "bare_rust_invocation:{program}"
            )));
        }
    }
    Ok(())
}
