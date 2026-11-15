// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::ReleaseError;
use crate::git_ref;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TagPattern(String);

impl TagPattern {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub(crate) fn render(template: &str, package: &str) -> Option<TagPattern> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(open) = rest.find(['{', '}']) {
        out.push_str(&rest[..open]);
        let tail = &rest[open..];
        let inner = tail.strip_prefix("{{")?;
        let close = inner.find("}}")?;
        match inner[..close].trim() {
            "package" => out.push_str(package),
            "version" => out.push_str("{{ version }}"),
            _ => return None,
        }
        rest = &inner[close + 2..];
    }
    out.push_str(rest);
    let example = out.replace("{{ version }}", "0.0.0-0+0");
    (is_tag_name(&example) && git_ref::is_valid(&format!("refs/tags/{example}")))
        .then_some(TagPattern(out))
}

#[must_use]
pub fn is_tag_name(text: &str) -> bool {
    !text.is_empty()
        && !text.starts_with(['.', '-'])
        && !text.ends_with('.')
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+'))
}

pub(crate) fn checked(
    template: &str,
    package: &str,
    root: &str,
    named: Option<&str>,
) -> Result<TagPattern, ReleaseError> {
    render(template, package).ok_or_else(|| ReleaseError::InvalidTagTemplate {
        root: root.to_owned(),
        package: named.map(str::to_owned),
        template: template.to_owned(),
    })
}
