// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};

use velnor_actions_contract::{HomeKey, Job, JobEnv, JobTimeout, Step};

use crate::RenderError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WasteRule {
    JobContextEnv,
    RustupAutoInstall,
    SplitToolsIdentity,
}

impl WasteRule {
    pub const ALL: &'static [Self] = &[
        Self::JobContextEnv,
        Self::RustupAutoInstall,
        Self::SplitToolsIdentity,
    ];

    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::JobContextEnv => "W2 job_context_env",
            Self::RustupAutoInstall => "W3 rustup_auto_install",
            Self::SplitToolsIdentity => "W7 split_tools_identity",
        }
    }

    #[must_use]
    pub const fn fix(self) -> &'static str {
        match self {
            Self::JobContextEnv => {
                "set all four toolchain homes to their standard values in every step that runs a toolchain command, and RUSTUP_HOME and CARGO_HOME in every Rust verifier step."
            }
            Self::RustupAutoInstall => {
                "bind the job to the rust toolchain job env; do not override RUSTUP_AUTO_INSTALL per step."
            }
            Self::SplitToolsIdentity => {
                "install the lane's union tool set in every rust job so the lane has one tools identity."
            }
        }
    }

    fn run(self, jobs: &BTreeMap<String, Job>) -> Vec<WasteFinding> {
        match self {
            Self::JobContextEnv => crate::waste_lint_env::w2(jobs),
            Self::RustupAutoInstall => crate::waste_lint_env::w3(jobs),
            Self::SplitToolsIdentity => crate::waste_lint_tools::w7(jobs),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ExecWord {
    Cargo,
    Rustup,
    Rustc,
    Mise,
    Mbx,
}

impl ExecWord {
    pub(crate) fn from_word(word: &str) -> Option<Self> {
        match word {
            "cargo" => Some(Self::Cargo),
            "rustup" => Some(Self::Rustup),
            "rustc" => Some(Self::Rustc),
            "mise" => Some(Self::Mise),
            "mbx" => Some(Self::Mbx),
            _ => None,
        }
    }

    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Cargo => "cargo",
            Self::Rustup => "rustup",
            Self::Rustc => "rustc",
            Self::Mise => "mise",
            Self::Mbx => "mbx",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum WasteFinding {
    MissingHome {
        job: String,
        step: String,
        word: ExecWord,
        missing: Vec<HomeKey>,
    },
    MissingVerifierHome {
        job: String,
        step: String,
        missing: Vec<HomeKey>,
    },
    ConflictingHome {
        job: String,
        step: String,
        key: HomeKey,
        expected: String,
        found: String,
    },
    AutoInstallNotDisabled {
        job: String,
        step: Option<String>,
        found: Option<String>,
    },
    ToolsIdentity(crate::waste_lint_tools::ToolsIdentityFinding),
}

impl WasteFinding {
    #[must_use]
    pub const fn rule(&self) -> WasteRule {
        match self {
            Self::MissingHome { .. }
            | Self::MissingVerifierHome { .. }
            | Self::ConflictingHome { .. } => WasteRule::JobContextEnv,
            Self::AutoInstallNotDisabled { .. } => WasteRule::RustupAutoInstall,
            Self::ToolsIdentity(_) => WasteRule::SplitToolsIdentity,
        }
    }
}

impl Display for WasteFinding {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: ", self.rule().id())?;
        match self {
            Self::MissingHome {
                job,
                step,
                word,
                missing,
            } => {
                let names: Vec<&str> = missing.iter().map(|key| key.env_name()).collect();
                write!(
                    f,
                    "job {job} step {step:?} runs {} without {}",
                    word.word(),
                    names.join(", ")
                )?;
            }
            Self::MissingVerifierHome { job, step, missing } => {
                let names: Vec<&str> = missing.iter().map(|key| key.env_name()).collect();
                write!(
                    f,
                    "job {job} verifier step {step:?} reads the toolchain without {}",
                    names.join(", ")
                )?;
            }
            Self::ConflictingHome {
                job,
                step,
                key,
                expected,
                found,
            } => write!(
                f,
                "job {job} step {step:?} sets {}={found} instead of {expected}",
                key.env_name()
            )?,
            Self::AutoInstallNotDisabled {
                job,
                step: None,
                found: _,
            } => write!(f, "job {job} has no job-level RUSTUP_AUTO_INSTALL=0")?,
            Self::AutoInstallNotDisabled {
                job,
                step: Some(step),
                found,
            } => write!(
                f,
                "job {job} step {step:?} sets RUSTUP_AUTO_INSTALL={}",
                found.as_deref().unwrap_or_default()
            )?,
            Self::ToolsIdentity(finding) => write!(f, "{finding}")?,
        }
        write!(f, ". fix: {}", self.rule().fix())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasteFindings(Vec<WasteFinding>);

impl WasteFindings {
    #[must_use]
    pub fn new(findings: Vec<WasteFinding>) -> Option<Self> {
        (!findings.is_empty()).then_some(Self(findings))
    }

    #[must_use]
    pub fn as_slice(&self) -> &[WasteFinding] {
        &self.0
    }
}

impl Display for WasteFindings {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        for (index, finding) in self.0.iter().enumerate() {
            if index > 0 {
                writeln!(f)?;
            }
            write!(f, "{finding}")?;
        }
        Ok(())
    }
}

#[must_use]
pub fn waste_findings(jobs: &BTreeMap<String, Job>) -> Vec<WasteFinding> {
    let mut findings: Vec<WasteFinding> = WasteRule::ALL
        .iter()
        .flat_map(|rule| rule.run(jobs))
        .collect();
    findings.sort();
    findings
}

pub(crate) fn check(jobs: &BTreeMap<String, Job>) -> Result<(), RenderError> {
    match WasteFindings::new(waste_findings(jobs)) {
        Some(findings) => Err(RenderError::WasteLint(findings)),
        None => Ok(()),
    }
}

pub(crate) fn bind_and_check(jobs: &mut BTreeMap<String, Job>) -> Result<(), RenderError> {
    crate::rust_job_env::bind(jobs);
    crate::cache_doctor::check(jobs)?;
    check(jobs)
}

pub(crate) fn bind_and_check_standalone(id: &str, steps: &[Step]) -> Result<JobEnv, RenderError> {
    let job = Job {
        display_name: id.to_owned(),
        runs_on: String::new(),
        check_runner: None,
        timeout_minutes: JobTimeout::DEFAULT,
        needs: Vec::new(),
        condition: None,
        permissions: None,
        cache_access: velnor_actions_contract::CacheAccess::Read,
        environment: None,
        services: Vec::new(),
        outputs: BTreeMap::new(),
        env: JobEnv::Unset,
        steps: steps.to_vec(),
    };
    let mut jobs = BTreeMap::from([(id.to_owned(), job)]);
    crate::rust_job_env::bind(&mut jobs);
    check(&jobs)?;
    Ok(jobs.get(id).map_or(JobEnv::Unset, |bound| bound.env))
}
