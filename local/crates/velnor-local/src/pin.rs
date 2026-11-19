// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use ring::digest::{SHA256, digest};

const PIN_VARIABLE: &str = "VELNOR_HELPER_SHA256:";
const LINUX_ARM64: &str = "''Linux'' && runner.arch == ''ARM64'' && ''";
const HEX: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sha256(String);

impl Sha256 {
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        let hex = digest(&SHA256, bytes)
            .as_ref()
            .iter()
            .flat_map(|byte| [byte >> 4, byte & 15])
            .filter_map(|nibble| char::from_digit(u32::from(nibble), 16))
            .collect();
        Self(hex)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[must_use]
pub fn rewrite(workflow: &str, pin: &Sha256) -> (String, usize) {
    let mut count = 0;
    let mut rewritten = String::with_capacity(workflow.len());
    for line in workflow.split_inclusive('\n') {
        match repin(line, pin) {
            Some(line) => {
                count += 1;
                rewritten.push_str(&line);
            }
            None => rewritten.push_str(line),
        }
    }
    (rewritten, count)
}

fn repin(line: &str, pin: &Sha256) -> Option<String> {
    if !line.contains(PIN_VARIABLE) {
        return None;
    }
    let start = line.find(LINUX_ARM64)? + LINUX_ARM64.len();
    let old = line.get(start..start + HEX)?;
    let closed = line.get(start + HEX..)?.starts_with("''");
    let hex = old
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    (closed && hex).then(|| format!("{}{}{}", &line[..start], pin.as_str(), &line[start + HEX..]))
}
