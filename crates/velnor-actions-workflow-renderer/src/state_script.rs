// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(crate) fn one_line(script: &str) -> String {
    let mut joined = String::new();
    for line in script
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if !joined.is_empty() {
            let opens = joined.ends_with('{')
                || ["then", "do", "else"]
                    .iter()
                    .any(|word| joined.ends_with(&format!(" {word}")) || joined == *word);
            joined.push_str(if opens { " " } else { "; " });
        }
        joined.push_str(line);
    }
    joined
}
