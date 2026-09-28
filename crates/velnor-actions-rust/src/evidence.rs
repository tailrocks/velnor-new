//! Strong executable-intent evidence and per-workspace profiles.

use std::fmt;

use velnor_actions_contract::MARKER_PREFIX;

use crate::scan::{
    has_adjacent, has_command, has_setting, line_no, snippet, starts_with_word, strip_comment,
};

/// Recommendation code for the default-runner notice.
pub const NEXTEST_RECOMMENDATION: &str = "nextest_recommendation";

/// Recommendation code for persisting a durable signal.
pub const PERSIST_EVIDENCE: &str = "persist_evidence";

/// Selected compile driver for one workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileDriver {
    /// Plain Cargo compilation.
    Cargo,
    /// MBX compilation.
    Mbx,
}

impl CompileDriver {
    /// Stable profile name.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Mbx => "mbx",
        }
    }
}

/// Selected test runner for one workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestRunner {
    /// Plain `cargo test`.
    CargoTest,
    /// Nextest execution.
    CargoNextest,
}

impl TestRunner {
    /// Stable profile name.
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CargoTest => "cargo_test",
            Self::CargoNextest => "cargo_nextest",
        }
    }
}

/// Evidence strength; only strong evidence selects profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EvidenceStrength {
    /// Executable project intent.
    Strong,
}

/// One recorded evidence sighting.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Evidence {
    /// Repository-relative evidence path.
    pub path: String,
    /// One-based line number.
    pub line: u32,
    /// Matched setting or command text.
    pub command_or_setting: String,
    /// Evidence strength.
    pub strength: EvidenceStrength,
}

/// Advisory recommendation emitted with a profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recommendation {
    /// Stable recommendation code.
    pub code: String,
    /// Human-readable guidance.
    pub message: String,
}

/// Per-workspace execution profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RustExecutionProfile {
    /// Selected compile driver.
    pub compile_driver: CompileDriver,
    /// Selected test runner.
    pub test_runner: TestRunner,
    /// Strong evidence backing the selection, sorted by `(path, line)`.
    pub evidence: Vec<Evidence>,
}

/// Detected profile plus advisory recommendations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileOutcome {
    /// Selected profile.
    pub profile: RustExecutionProfile,
    /// Advisory recommendations.
    pub recommendations: Vec<Recommendation>,
}

/// One named content blob offered as evidence.
#[derive(Debug, Clone, Copy)]
pub struct EvidenceFile<'a> {
    /// Repository-relative path the content was read from.
    pub path: &'a str,
    /// File content.
    pub content: &'a str,
}

/// Inputs for profile detection (all bytes come from the orchestrator).
#[derive(Debug, Clone, Default)]
pub struct ProfileInputs<'a> {
    /// Tool-config content holding the Rust `mr_boxington` setting, if any.
    pub tool_config: Option<EvidenceFile<'a>>,
    /// Cargo config contents inspected for `rustc-wrapper` naming MBX.
    pub cargo_configs: Vec<EvidenceFile<'a>>,
    /// Executable task and script contents.
    pub executables: Vec<EvidenceFile<'a>>,
    /// Hand-written workflow contents (never generated output).
    pub handwritten_workflows: Vec<EvidenceFile<'a>>,
}

/// Profile detection failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    /// Both test runners are explicitly used.
    AmbiguousTestRunner {
        /// Conflicting evidence, sorted.
        evidence: Vec<Evidence>,
    },
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self::AmbiguousTestRunner { evidence } = self;
        write!(f, "ambiguous_test_runner: {} sightings", evidence.len())
    }
}

impl std::error::Error for ProfileError {}

/// Detect the per-workspace profile from strong evidence only.
///
/// # Errors
///
/// Returns [`ProfileError::AmbiguousTestRunner`] when both test runners are
/// explicitly used.
pub fn detect_profile(inputs: &ProfileInputs<'_>) -> Result<ProfileOutcome, ProfileError> {
    let mut mbx = Vec::new();
    let mut nextest = Vec::new();
    let mut cargo_test = Vec::new();
    let mut fragile_only = true;
    if let Some(config) = &inputs.tool_config {
        let found = scan_tool_config(config);
        if !found.is_empty() {
            fragile_only = false;
        }
        mbx.extend(found);
    }
    for config in &inputs.cargo_configs {
        let found = scan_cargo_config(config);
        if !found.is_empty() {
            fragile_only = false;
        }
        mbx.extend(found);
    }
    for file in &inputs.executables {
        let (found_mbx, found_nextest, found_cargo) = scan_command_text(file);
        if !found_mbx.is_empty() || !found_nextest.is_empty() || !found_cargo.is_empty() {
            fragile_only = false;
        }
        mbx.extend(found_mbx);
        nextest.extend(found_nextest);
        cargo_test.extend(found_cargo);
    }
    for file in &inputs.handwritten_workflows {
        let (found_mbx, found_nextest, found_cargo) = scan_command_text(file);
        mbx.extend(found_mbx);
        nextest.extend(found_nextest);
        cargo_test.extend(found_cargo);
    }
    if !nextest.is_empty() && !cargo_test.is_empty() {
        let mut conflicting = nextest;
        conflicting.extend(cargo_test);
        conflicting.sort();
        return Err(ProfileError::AmbiguousTestRunner {
            evidence: conflicting,
        });
    }
    let compile_driver = if mbx.is_empty() {
        CompileDriver::Cargo
    } else {
        CompileDriver::Mbx
    };
    let test_defaulted = nextest.is_empty() && cargo_test.is_empty();
    let test_runner = if nextest.is_empty() {
        TestRunner::CargoTest
    } else {
        TestRunner::CargoNextest
    };
    let mut evidence = mbx;
    evidence.extend(nextest);
    evidence.extend(cargo_test);
    evidence.sort();
    let empty = evidence.is_empty();
    let recommendations = recommend(test_defaulted, fragile_only, empty);
    Ok(ProfileOutcome {
        profile: RustExecutionProfile {
            compile_driver,
            test_runner,
            evidence,
        },
        recommendations,
    })
}

fn recommend(test_defaulted: bool, fragile_only: bool, empty: bool) -> Vec<Recommendation> {
    let mut out = Vec::new();
    if test_defaulted {
        out.push(Recommendation {
            code: NEXTEST_RECOMMENDATION.to_owned(),
            message: "no explicit test-runner usage; defaulting to `cargo test`".to_owned(),
        });
    }
    if fragile_only {
        let message = if empty {
            "no MBX or Nextest usage detected; to adopt either, add a durable signal"
        } else {
            "profile rests on hand-written workflows that generation replaces; persist a durable signal"
        };
        out.push(Recommendation {
            code: PERSIST_EVIDENCE.to_owned(),
            message: message.to_owned(),
        });
    }
    out
}

/// Whether content is generated output (never evidence).
#[must_use]
pub fn is_generated_output(content: &str) -> bool {
    content
        .lines()
        .next()
        .is_some_and(|first| first.starts_with(MARKER_PREFIX))
}

/// Whether `path` must be skipped by evidence collection (output, docs, locks, caches).
#[must_use]
pub fn evidence_scan_excluded(path: &str) -> bool {
    if path == ".github" || path.starts_with(".github/") {
        return true;
    }
    if is_lock_path(path) {
        return true;
    }
    if is_doc_path(path) {
        return true;
    }
    if path == "target" || path.starts_with("target/") || path.contains("/target/") {
        return true;
    }
    path == ".git" || path.starts_with(".git/")
}

/// Whether `path` is documentation (never evidence).
fn is_doc_path(path: &str) -> bool {
    if path == "docs" || path.starts_with("docs/") {
        return true;
    }
    path.rsplit('/').next().is_some_and(|file| {
        file.starts_with("README")
            || ["md", "mdx", "markdown", "rst", "adoc"]
                .iter()
                .any(|extension| has_extension(file, extension))
    })
}

/// Whether `file` carries `extension` (ASCII case-insensitive).
fn has_extension(file: &str, extension: &str) -> bool {
    std::path::Path::new(file)
        .extension()
        .is_some_and(|found| found.eq_ignore_ascii_case(extension))
}

/// Whether `path` is a lockfile (never evidence).
fn is_lock_path(path: &str) -> bool {
    path.rsplit('/').next().is_some_and(|file| {
        file == "Cargo.lock" || file == "mise.lock" || has_extension(file, "lock")
    })
}

/// Scan tool-config content for the Rust `mr_boxington` setting.
fn scan_tool_config(file: &EvidenceFile<'_>) -> Vec<Evidence> {
    if is_generated_output(file.content) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (index, line) in file.content.lines().enumerate() {
        let code = strip_comment(line);
        if has_setting(code, "mr_boxington", "true") {
            out.push(sighting(file, index, code));
        }
    }
    out
}

/// Scan Cargo config content for `rustc-wrapper` naming MBX.
fn scan_cargo_config(file: &EvidenceFile<'_>) -> Vec<Evidence> {
    if is_generated_output(file.content) {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (index, line) in file.content.lines().enumerate() {
        let code = strip_comment(line);
        if mentions_wrapper(code) && mentions_mbx(code) {
            out.push(sighting(file, index, code));
        }
    }
    out
}

fn mentions_wrapper(code: &str) -> bool {
    code.contains("rustc-wrapper")
        || code.contains("rustc_wrapper")
        || code.contains("RUSTC_WRAPPER")
}

fn mentions_mbx(code: &str) -> bool {
    has_command(code, "mbx") || code.contains("boxington")
}

/// Scan task, script, or workflow text for driver and runner invocations.
fn scan_command_text(file: &EvidenceFile<'_>) -> (Vec<Evidence>, Vec<Evidence>, Vec<Evidence>) {
    if is_generated_output(file.content) {
        return (Vec::new(), Vec::new(), Vec::new());
    }
    let mut mbx = Vec::new();
    let mut nextest = Vec::new();
    let mut cargo_test = Vec::new();
    for (index, line) in file.content.lines().enumerate() {
        let code = strip_comment(line);
        if code.trim().is_empty() {
            continue;
        }
        if invokes_mbx(code) {
            mbx.push(sighting(file, index, code));
        }
        if invokes_nextest(code) {
            nextest.push(sighting(file, index, code));
        }
        if invokes_cargo_test(code) {
            cargo_test.push(sighting(file, index, code));
        }
    }
    (mbx, nextest, cargo_test)
}

fn invokes_mbx(code: &str) -> bool {
    has_command(code, "mbx")
        || has_command(code, "mr-boxington")
        || has_command(code, "mr_boxington")
}

fn invokes_nextest(code: &str) -> bool {
    if has_adjacent(code, "cargo", "nextest") || has_command(code, "cargo-nextest") {
        return true;
    }
    let mut search = code;
    while let Some(pos) = crate::scan::find_word(search, "nextest") {
        let rest = search[pos + "nextest".len()..].trim_start();
        if starts_with_word(rest, "run")
            || starts_with_word(rest, "archive")
            || starts_with_word(rest, "list")
        {
            return true;
        }
        search = &search[pos + "nextest".len()..];
    }
    false
}

fn invokes_cargo_test(code: &str) -> bool {
    has_adjacent(code, "cargo", "test") || has_adjacent(code, "mbx", "test")
}

fn sighting(file: &EvidenceFile<'_>, index: usize, code: &str) -> Evidence {
    Evidence {
        path: file.path.to_owned(),
        line: line_no(index),
        command_or_setting: snippet(code),
        strength: EvidenceStrength::Strong,
    }
}
