// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PackageIdentity<'a> {
    pub source: Option<&'a str>,
    pub name: &'a str,
    pub version: &'a str,
}

impl<'a> PackageIdentity<'a> {
    #[must_use]
    pub fn parse(package_id: &'a str) -> Option<Self> {
        let (source, name, version) = if let Some((spec, source)) = package_id
            .strip_suffix(')')
            .and_then(|legacy| legacy.rsplit_once(" ("))
        {
            let (name, version) = legacy(spec)?;
            (Some(source), name, version)
        } else if let Some((source, spec)) = package_id.rsplit_once('#') {
            let (name, version) = spec.rsplit_once('@').or_else(|| {
                let path = source.split_once('?').map_or(source, |(path, _)| path);
                path.rsplit('/')
                    .find(|part| !part.is_empty())
                    .map(|name| (name, spec))
            })?;
            (Some(source), name, version)
        } else {
            let (name, version) = legacy(package_id)?;
            (None, name, version)
        };
        if source.is_some_and(str::is_empty) || name.is_empty() || version.is_empty() {
            return None;
        }
        Some(Self {
            source,
            name,
            version,
        })
    }
}

fn legacy(spec: &str) -> Option<(&str, &str)> {
    let mut fields = spec.split_whitespace();
    let name = fields.next()?;
    let version = fields.next()?;
    fields.next().is_none().then_some((name, version))
}
