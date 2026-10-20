//! One Gradle root, analysed statically.
//!
//! Combines the wrapper gates, the settings model, the project edges and the
//! dependency-integrity check into one result with a flat, sorted list of
//! finding codes. The caller supplies the file list and a reader, so the same
//! function serves generation (working tree), planning (git objects) and tests.

use std::collections::{BTreeMap, BTreeSet};

use crate::edges::project_edges;
use crate::integrity::{VERIFICATION_METADATA, check_locking, check_verification};
use crate::settings::{self, SettingsModel};
use crate::wrapper::{self, JAR_PATH, PROPERTIES_PATH, VerifiedWrapper};

/// Result of analysing one root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootAnalysis {
    /// Root directory relative to the repository (`""` for the repository root).
    pub root: String,
    /// The verified wrapper, when every gate passed.
    pub wrapper: Option<VerifiedWrapper>,
    /// The static settings model, when readable.
    pub settings: Option<SettingsModel>,
    /// Project path to the project paths its build script depends on.
    pub edges: BTreeMap<String, BTreeSet<String>>,
    /// Sorted, de-duplicated finding codes; empty means the root is acceptable.
    pub findings: Vec<String>,
}

fn settings_name(files: &BTreeSet<String>) -> Option<&'static str> {
    ["settings.gradle.kts", "settings.gradle"]
        .into_iter()
        .find(|name| files.contains(*name))
}

/// Analyse a root. `files` and `read` use root-relative `/` paths.
#[must_use]
pub fn analyze_root(
    root: &str,
    files: &BTreeSet<String>,
    read: &dyn Fn(&str) -> Option<Vec<u8>>,
) -> RootAnalysis {
    let text = |path: &str| read(path).and_then(|bytes| String::from_utf8(bytes).ok());
    let mut findings: BTreeSet<String> = BTreeSet::new();
    let mut analysis = RootAnalysis {
        root: root.to_owned(),
        wrapper: None,
        settings: None,
        edges: BTreeMap::new(),
        findings: Vec::new(),
    };
    let properties = text(PROPERTIES_PATH);
    let jar = read(JAR_PATH).unwrap_or_default();
    match wrapper::verify(properties.as_deref(), &jar) {
        Ok(verified) => analysis.wrapper = Some(verified),
        Err(finding) => {
            findings.insert(finding.code().to_owned());
        }
    }
    let Some(name) = settings_name(files) else {
        findings.insert("gradle_settings_missing".to_owned());
        analysis.findings = findings.into_iter().collect();
        return analysis;
    };
    match settings::parse(&text(name).unwrap_or_default()) {
        Ok(model) => analysis.settings = Some(model),
        Err(dynamic) => {
            findings.insert(format!("{}:{}", dynamic.code(), dynamic.reason));
        }
    }
    if let Some(model) = &analysis.settings {
        let pairs: Vec<(String, String)> = model
            .projects
            .iter()
            .map(|project| (project.path.clone(), project.dir.clone()))
            .collect();
        let known: BTreeSet<String> = pairs.iter().map(|(path, _)| path.clone()).collect();
        for (path, dir) in &pairs {
            let script = ["build.gradle.kts", "build.gradle"]
                .iter()
                .find_map(|file| {
                    let full = format!("{dir}/{file}");
                    files.contains(&full).then(|| text(&full)).flatten()
                });
            let deps = script.map_or_else(BTreeSet::new, |script| project_edges(&script, &known));
            analysis.edges.insert(path.clone(), deps);
        }
        findings.extend(
            check_locking(&pairs, files, &text)
                .iter()
                .map(crate::integrity::IntegrityFinding::code),
        );
    }
    let metadata = files
        .contains(VERIFICATION_METADATA)
        .then(|| text(VERIFICATION_METADATA))
        .flatten();
    findings.extend(
        check_verification(metadata.as_deref())
            .iter()
            .map(crate::integrity::IntegrityFinding::code),
    );
    analysis.findings = findings.into_iter().collect();
    analysis
}
