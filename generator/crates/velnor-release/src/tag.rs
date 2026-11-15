// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::error::ReleaseError;
use crate::git_ref;

pub(crate) const VERSION: &str = "{{ version }}";
pub(crate) const PACKAGE_WILDCARD: &str = "{{ package }}";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TagPattern(String);

impl TagPattern {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub(crate) fn may_collide(&self, other: &TagPattern) -> bool {
        let (first, second) = (self.as_str(), other.as_str());
        let prefix = |text: &str| text.split(VERSION).next().unwrap_or_default().to_owned();
        let fixed = |text: &str| !text.contains(VERSION);
        let (short, long) = if prefix(first).len() <= prefix(second).len() {
            (first, second)
        } else {
            (second, first)
        };
        let (short_prefix, long_prefix) = (prefix(short), prefix(long));
        let Some(rest) = long_prefix.strip_prefix(short_prefix.as_str()) else {
            return false;
        };
        if fixed(short) {
            return fixed(long) && short == long;
        }
        let version_like = |text: &str| {
            text.is_empty()
                || (text.starts_with(|c: char| c.is_ascii_digit())
                    && text
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+')))
        };
        version_like(rest)
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
            "version" => out.push_str(VERSION),
            _ => return None,
        }
        rest = &inner[close + 2..];
    }
    out.push_str(rest);
    let example = out
        .replace(VERSION, "0.0.0-0+0")
        .replace(PACKAGE_WILDCARD, "package");
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
    let pattern = render(template, package).ok_or_else(|| ReleaseError::InvalidTagTemplate {
        root: root.to_owned(),
        package: named.map(str::to_owned),
        template: template.to_owned(),
    })?;
    if pattern.as_str().contains("{{ version }}") {
        Ok(pattern)
    } else {
        Err(ReleaseError::TagTemplateWithoutVersion {
            root: root.to_owned(),
            package: named.map(str::to_owned),
        })
    }
}
