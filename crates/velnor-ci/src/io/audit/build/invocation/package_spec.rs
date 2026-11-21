// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::RustMember;

pub(super) struct Spec<'a> {
    name: &'a str,
    version: Option<&'a str>,
}

impl Spec<'_> {
    pub(super) fn matches(&self, member: &RustMember) -> bool {
        member.name.as_str() == self.name
            && self
                .version
                .is_none_or(|version| member.version.as_deref() == Some(version))
    }
}

pub(super) fn parse(spec: &str) -> Option<Spec<'_>> {
    let Some((scheme, location)) = spec.split_once("://") else {
        return if spec.contains('#') {
            None
        } else {
            Some(versioned(spec))
        };
    };
    if scheme.strip_prefix("path+").unwrap_or(scheme) != "file" {
        return None;
    }
    let (address, fragment) = location
        .split_once('#')
        .map_or((location, None), |(address, fragment)| {
            (address, Some(fragment))
        });
    match fragment {
        Some(fragment) if !fragment.starts_with(|c: char| c.is_ascii_digit()) => {
            Some(versioned(fragment))
        }
        Some(version) => Some(Spec {
            name: last_segment(address),
            version: Some(version),
        }),
        None => Some(Spec {
            name: last_segment(address),
            version: None,
        }),
    }
}

fn versioned(spec: &str) -> Spec<'_> {
    match spec.split_once(['@', ':']) {
        Some((name, version)) => Spec {
            name,
            version: Some(version),
        },
        None => Spec {
            name: spec,
            version: None,
        },
    }
}

fn last_segment(address: &str) -> &str {
    address.rsplit('/').next().unwrap_or(address)
}
