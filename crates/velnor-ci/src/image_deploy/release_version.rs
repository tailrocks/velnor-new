// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use super::promote::is_version;
use crate::error::CiError;

#[derive(Debug)]
pub struct Release<'a> {
    pub version: &'a str,
    pub bins: &'a [String],
    pub package: Option<&'a str>,
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}

#[derive(Deserialize)]
struct Package {
    name: String,
    version: String,
    targets: Vec<Target>,
}

#[derive(Deserialize)]
struct Target {
    name: String,
    kind: Vec<String>,
}

impl Package {
    fn builds(&self, bin: &str) -> bool {
        self.targets
            .iter()
            .any(|target| target.name == bin && target.kind.iter().any(|kind| kind == "bin"))
    }
}

pub fn check(metadata: &[u8], release: &Release<'_>) -> Result<String, CiError> {
    let Release {
        version,
        bins,
        package,
    } = release;
    if !is_version(version) {
        return Err(CiError::new(format!("`{version}` is not a version X.Y.Z")));
    }
    let metadata: Metadata = serde_json::from_slice(metadata)
        .map_err(|error| CiError::new(format!("not the output of cargo metadata: {error}")))?;
    let candidates: Vec<&Package> = metadata
        .packages
        .iter()
        .filter(|candidate| package.is_none_or(|name| candidate.name == name))
        .collect();
    if let Some(name) = package
        && candidates.is_empty()
    {
        return Err(CiError::new(format!(
            "package {name} is not a member of the workspace at the tagged commit"
        )));
    }
    let mut owners: Vec<&Package> = Vec::new();
    for bin in *bins {
        let found: Vec<&Package> = candidates
            .iter()
            .copied()
            .filter(|candidate| candidate.builds(bin))
            .collect();
        match found[..] {
            [] => {
                return Err(CiError::new(format!(
                    "no package of the workspace at the tagged commit builds the bin {bin}"
                )));
            }
            [one] => {
                if !owners.iter().any(|owner| owner.name == one.name) {
                    owners.push(one);
                }
            }
            _ => {
                let names: Vec<&str> = found.iter().map(|owner| owner.name.as_str()).collect();
                return Err(CiError::new(format!(
                    "the bin {bin} is built by several packages ({}); set `package` on the image to say which one owns it",
                    names.join(", ")
                )));
            }
        }
    }
    let [owner] = owners[..] else {
        let names: Vec<&str> = owners.iter().map(|owner| owner.name.as_str()).collect();
        return Err(CiError::new(format!(
            "the bins {} are built by different packages ({}), so no one crate version can name the release",
            bins.join(", "),
            names.join(", ")
        )));
    };
    if owner.version != *version {
        return Err(CiError::new(format!(
            "the release tag names version {version}, but package {}, which builds {}, is at version {} at the tagged commit; nothing was promoted, so delete the mistyped tag and tag the version the crate holds, or bump the crate first",
            owner.name,
            bins.join(", "),
            owner.version
        )));
    }
    Ok(format!(
        "package {} is at version {version}, as the release tag names\n",
        owner.name
    ))
}
