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
    pub(crate) fn instantiate(&self, package: &str) -> TagPattern {
        TagPattern(self.0.replace(PACKAGE_WILDCARD, package))
    }

    #[must_use]
    pub(crate) fn ends_with_version(&self) -> bool {
        self.0.ends_with(VERSION)
    }

    #[must_use]
    pub(crate) fn may_collide(&self, other: &TagPattern) -> bool {
        match (self.0.split_once(VERSION), other.0.split_once(VERSION)) {
            (None, None) => self.0 == other.0,
            (Some(variable), None) => fixed_renders(&other.0, variable),
            (None, Some(variable)) => fixed_renders(&self.0, variable),
            (Some(first), Some(second)) => variable_overlap(first, second),
        }
    }
}

fn version_chars(text: &str) -> bool {
    text.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
}

fn version_like(text: &str) -> bool {
    text.starts_with(|c: char| c.is_ascii_digit()) && version_chars(text)
}

fn fixed_renders(fixed: &str, (prefix, suffix): (&str, &str)) -> bool {
    fixed
        .strip_prefix(prefix)
        .and_then(|rest| rest.strip_suffix(suffix))
        .is_some_and(version_like)
}

fn variable_overlap(first: (&str, &str), second: (&str, &str)) -> bool {
    let ((short, short_suffix), (long, long_suffix)) = if first.0.len() <= second.0.len() {
        (first, second)
    } else {
        (second, first)
    };
    let Some(prefix_gap) = long.strip_prefix(short) else {
        return false;
    };
    let suffix_gap = long_suffix
        .strip_suffix(short_suffix)
        .or_else(|| short_suffix.strip_suffix(long_suffix));
    suffix_gap.is_some_and(|gap| {
        (prefix_gap.is_empty() || version_like(prefix_gap)) && version_chars(gap)
    })
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
    if out.matches(VERSION).count() > 1 {
        return None;
    }
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
