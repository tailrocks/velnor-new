// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

const MAX_LINE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CargoArtifact {
    pub package_id: String,
    pub manifest_path: String,
    pub target: CargoTarget,
    pub profile: CargoProfile,
    pub features: Vec<String>,
    pub filenames: Vec<String>,
    pub fresh: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CargoTarget {
    pub name: String,
    pub kind: Vec<String>,
    pub crate_types: Vec<String>,
    pub src_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CargoProfile {
    pub opt_level: String,
    pub debuginfo: serde_json::Value,
    pub debug_assertions: bool,
    pub overflow_checks: bool,
    pub test: bool,
}

/// A build script cargo ran or replayed (`build-script-executed`): its package and output
/// directory, whose parent directory holds the run's records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildScriptRun {
    pub package_id: String,
    pub out_dir: String,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ArtifactBatch {
    pub artifacts: Vec<CargoArtifact>,
    pub build_scripts: Vec<BuildScriptRun>,
    pub rejected: u64,
    pub progress: Vec<CargoProgress>,
    pub tests: Vec<(String, f64)>,
    /// Units cargo reported a compile error for.
    pub failed_units: Vec<FailedUnit>,
    /// Cargo reported `build-finished` with `success: false`. A diagnostic only: it does not say
    /// which units failed.
    pub build_failed: bool,
}

/// A unit that cargo could not compile. Cargo's text report (`error: could not compile`) names the
/// package only, so `version` is set only when a JSON `compiler-message` gave the package id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedUnit {
    pub package: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CargoProgress {
    pub kind: ProgressKind,
    pub package: String,
    pub version: String,
    pub location: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProgressKind {
    Compiling,
    Checking,
    Documenting,
    Downloaded,
}

#[derive(Debug, Default)]
pub struct ArtifactTap {
    line: Vec<u8>,
    discarding: bool,
    batch: ArtifactBatch,
}

impl ArtifactTap {
    #[must_use]
    pub fn take_ready(&mut self) -> ArtifactBatch {
        std::mem::take(&mut self.batch)
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        for segment in bytes.split_inclusive(|byte| *byte == b'\n') {
            let newline = segment.last() == Some(&b'\n');
            let content = if newline {
                &segment[..segment.len() - 1]
            } else {
                segment
            };
            if !self.discarding {
                if content.len() > MAX_LINE_BYTES - self.line.len() {
                    self.batch.rejected += 1;
                    self.line.clear();
                    self.discarding = true;
                } else {
                    let required = self.line.len() + content.len();
                    if required > self.line.capacity() {
                        self.line
                            .reserve_exact(required.next_power_of_two() - self.line.len());
                    }
                    self.line.extend_from_slice(content);
                }
            }
            if newline {
                self.read_line();
                self.line.clear();
                self.discarding = false;
            }
        }
    }

    #[must_use]
    pub fn finish(mut self) -> ArtifactBatch {
        self.read_line();
        self.batch
    }

    fn read_line(&mut self) {
        if self.discarding {
            return;
        }
        if let Ok(line) = std::str::from_utf8(&self.line) {
            let line = crate::logscan::strip_ansi(crate::logscan::strip_timestamp(line));
            let line = line.trim_start();
            if let Some(progress) = cargo_progress(line) {
                self.batch.progress.push(progress);
            }
            if let Some(test) = individual_test(line) {
                self.batch.tests.push(test);
            }
            if let Some(unit) = failed_unit(line) {
                self.batch.failed_units.push(unit);
            }
        }
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&self.line) else {
            return;
        };
        match value.get("reason").and_then(serde_json::Value::as_str) {
            Some("compiler-message") => {
                let is_error = value
                    .pointer("/message/level")
                    .and_then(serde_json::Value::as_str)
                    == Some("error");
                let identity = value
                    .get("package_id")
                    .and_then(serde_json::Value::as_str)
                    .and_then(crate::cargo_package::PackageIdentity::parse);
                if is_error && let Some(identity) = identity {
                    self.batch.failed_units.push(FailedUnit {
                        package: identity.name.to_owned(),
                        version: Some(identity.version.to_owned()),
                    });
                }
                return;
            }
            Some("build-finished") => {
                if value.get("success").and_then(serde_json::Value::as_bool) == Some(false) {
                    self.batch.build_failed = true;
                }
                return;
            }
            Some("build-script-executed") => {
                let text = |key| value.get(key).and_then(serde_json::Value::as_str);
                if let (Some(package_id), Some(out_dir)) = (text("package_id"), text("out_dir")) {
                    self.batch.build_scripts.push(BuildScriptRun {
                        package_id: package_id.to_owned(),
                        out_dir: out_dir.to_owned(),
                    });
                }
                return;
            }
            Some("compiler-artifact") => {}
            _ => return,
        }
        match serde_json::from_value(value) {
            Ok(artifact) => self.batch.artifacts.push(artifact),
            Err(_) => self.batch.rejected += 1,
        }
    }
}

/// `error: could not compile `pkg` (lib) due to 1 previous error`, and the same with `(bin "x")`,
/// `(test "y")`, `due to previous error`, `; 2 previous errors`. Cargo prints it in every message
/// format, including `json-render-diagnostics`, which emits no `compiler-message` for rustc errors.
fn failed_unit(line: &str) -> Option<FailedUnit> {
    let rest = line.strip_prefix("error: could not compile `")?;
    let (package, _) = rest.split_once('`')?;
    (!package.is_empty()).then(|| FailedUnit {
        package: package.to_owned(),
        version: None,
    })
}

fn cargo_progress(line: &str) -> Option<CargoProgress> {
    let (rest, kind) = [
        ("Compiling ", ProgressKind::Compiling),
        ("Checking ", ProgressKind::Checking),
        ("Documenting ", ProgressKind::Documenting),
        ("Downloaded ", ProgressKind::Downloaded),
    ]
    .into_iter()
    .find_map(|(prefix, kind)| line.strip_prefix(prefix).map(|rest| (rest, kind)))?;
    let mut words = rest.splitn(3, char::is_whitespace);
    let package = words.next()?;
    let version = words.next()?.strip_prefix('v')?;
    let core = version.split(['-', '+']).next()?;
    let parts: Vec<_> = core.split('.').collect();
    if package.is_empty()
        || parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    let location = match words.next() {
        Some(suffix) => Some(
            suffix
                .trim()
                .strip_prefix('(')?
                .strip_suffix(')')?
                .to_owned(),
        ),
        None => None,
    };
    Some(CargoProgress {
        kind,
        package: package.to_owned(),
        version: version.to_owned(),
        location,
    })
}

fn individual_test(line: &str) -> Option<(String, f64)> {
    let line = if let Some(retry) = line.strip_prefix("TRY ") {
        let (attempt, status) = retry.split_once(' ')?;
        if !attempt.bytes().all(|byte| byte.is_ascii_digit())
            || !attempt.bytes().any(|byte| byte != b'0')
        {
            return None;
        }
        status
    } else {
        line
    };
    let rest = line
        .strip_prefix("PASS [")
        .or_else(|| line.strip_prefix("FAIL ["))
        .or_else(|| line.strip_prefix("LEAK ["))
        .or_else(|| line.strip_prefix("LEAK-FAIL ["))?;
    let (seconds, rest) = rest.split_once("s]")?;
    let seconds = seconds.trim().parse::<f64>().ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('(').map_or(Some(rest), |rest| {
        rest.split_once(')').map(|(_, rest)| rest.trim_start())
    })?;
    (!rest.is_empty()).then(|| (rest.to_owned(), seconds))
}
