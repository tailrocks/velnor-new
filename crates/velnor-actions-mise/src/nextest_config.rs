use std::collections::BTreeMap;
use std::fmt;

use crate::toml_scan::parse_toml;

pub const NEXTEST_CONFIG_REL: &str = ".config/nextest.toml";

pub const CI_PROFILE_NAME: &str = "ci";

pub const DEFAULT_PROFILE_NAME: &str = "default";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextestConfig {
    pub profiles: Vec<String>,
    pub ci_line: Option<u32>,
}

impl NextestConfig {
    #[must_use]
    pub fn has_ci_profile(&self) -> bool {
        self.ci_line.is_some()
    }

    #[must_use]
    pub fn selected_profile(&self) -> &'static str {
        if self.has_ci_profile() {
            CI_PROFILE_NAME
        } else {
            DEFAULT_PROFILE_NAME
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextestDiagnostic {
    pub line: u32,
    pub problem: String,
}

impl fmt::Display for NextestDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "nextest_config_invalid:{}:{}", self.line, self.problem)
    }
}

impl std::error::Error for NextestDiagnostic {}

pub fn parse_nextest_config(content: &str) -> Result<NextestConfig, NextestDiagnostic> {
    let doc = parse_toml(content).map_err(|failed| NextestDiagnostic {
        line: failed.line,
        problem: failed.problem,
    })?;
    let mut lines: BTreeMap<String, u32> = BTreeMap::new();
    for (path, line) in &doc.sections {
        record_profile(&mut lines, path, *line);
    }
    for item in &doc.assignments {
        if item.path.len() >= 3 {
            record_profile(&mut lines, &item.path[..2], item.line);
        }
    }
    let mut profiles: Vec<String> = lines.keys().cloned().collect();
    profiles.sort();
    Ok(NextestConfig {
        profiles,
        ci_line: lines.get(CI_PROFILE_NAME).copied(),
    })
}

fn record_profile(lines: &mut BTreeMap<String, u32>, path: &[String], line: u32) {
    if path.len() < 2 || path[0] != "profile" {
        return;
    }
    lines.entry(path[1].clone()).or_insert(line);
}
