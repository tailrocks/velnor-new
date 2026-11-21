// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) fn package_name(spec: &str) -> Option<&str> {
    let Some((scheme, location)) = spec.split_once("://") else {
        return if spec.contains('#') {
            None
        } else {
            versioned_name(spec)
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
            versioned_name(fragment)
        }
        _ => address.rsplit('/').next(),
    }
}

fn versioned_name(spec: &str) -> Option<&str> {
    spec.split(['@', ':']).next()
}
