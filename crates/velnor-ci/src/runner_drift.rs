// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::Deserialize;

use crate::error::CiError;
use crate::github::{GitHub, parse};

pub const LATEST_PATH: &str = "/repos/actions/runner/releases/latest";

pub const UNKNOWN_EXIT: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Version(u64, u64, u64);

impl Version {
    fn parse(text: &str) -> Result<Self, CiError> {
        let bare = text.strip_prefix('v').unwrap_or(text);
        let mut parts = bare.split('.').map(|part| {
            let canonical = !part.is_empty()
                && part.bytes().all(|byte| byte.is_ascii_digit())
                && (part == "0" || !part.starts_with('0'));
            if canonical {
                part.parse::<u64>().ok()
            } else {
                None
            }
        });
        match (parts.next(), parts.next(), parts.next(), parts.next()) {
            (Some(Some(major)), Some(Some(minor)), Some(Some(patch)), None) => {
                Ok(Self(major, minor, patch))
            }
            _ => Err(CiError::new(format!(
                "cannot read {text:?} as an actions/runner version X.Y.Z"
            ))),
        }
    }

    fn render(self) -> String {
        format!("{}.{}.{}", self.0, self.1, self.2)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    Current(String),
    Behind { reported: String, latest: String },
}

impl Drift {
    #[must_use]
    pub fn line(&self) -> String {
        match self {
            Self::Current(version) => format!("runner protocol current: {version}"),
            Self::Behind { reported, latest } => {
                format!("runner protocol behind: {reported} < {latest}")
            }
        }
    }

    #[must_use]
    pub fn behind(&self) -> bool {
        matches!(self, Self::Behind { .. })
    }
}

pub fn compare(reported: &str, latest_tag: &str) -> Result<Drift, CiError> {
    let reported = Version::parse(reported)?;
    let latest = Version::parse(latest_tag)?;
    Ok(if reported < latest {
        Drift::Behind {
            reported: reported.render(),
            latest: latest.render(),
        }
    } else {
        Drift::Current(reported.render())
    })
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

pub fn latest_tag(github: &impl GitHub) -> Result<String, CiError> {
    let release: Release = parse(&github.get(LATEST_PATH)?, "actions/runner latest release")?;
    Ok(release.tag_name)
}
