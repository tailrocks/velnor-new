// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::digest_b3;
use crate::errors::ContractError;

pub const CI_WORKFLOW_PATH: &str = ".github/workflows/ci.yml";

pub const STALE_WORKFLOW_PATHS: [&str; 1] = [".github/workflows/velnor.yml"];

pub const WORKFLOW_DISPLAY_NAME: &str = "CI";

pub const FRESHNESS_WORKFLOW_PATH: &str = ".github/workflows/freshness.yml";

pub const TOFU_APPLY_WORKFLOW_PATH: &str = ".github/workflows/tofu-apply.yml";

pub const PLAN_JOB_ID: &str = "plan";

pub const REQUIRED_JOB_ID: &str = "required";

pub const REQUIRED_GATE_JOB_ID: &str = "required-gate";

pub const PLAN_DISPLAY_NAME: &str = "Plan";

pub const REQUIRED_DISPLAY_NAME: &str = "Required";

pub const REQUIRED_VERDICT_DISPLAY_NAME: &str = "Required verdict";

pub const REQUIRED_CONDITION: &str = "${{ !cancelled() }}";

pub const REQUIRED_GATE_CONDITION: &str = "${{ always() }}";

pub const REQUIRED_GATE_FAIL_CONDITION: &str =
    "${{ always() && needs.required.result != 'success' }}";

pub const FRESHNESS_CRON_WEEKLY: &str = "0 6 * * 1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidatorKind {
    Alint,

    CargoDeny,

    CargoMachete,

    Jactionlint,

    Markdownlint,

    StrictJson,

    FrontmatterId,

    LinkCheck,

    NativeValidators,
}

impl ValidatorKind {
    #[must_use]
    pub fn job_id(&self) -> &'static str {
        match self {
            Self::Alint => "alint",
            Self::CargoDeny => "cargo-deny",
            Self::CargoMachete => "cargo-machete",
            Self::Jactionlint => "jactionlint",
            Self::Markdownlint => "markdownlint",
            Self::StrictJson => "strict-json",
            Self::FrontmatterId => "frontmatter-id",
            Self::LinkCheck => "link-check",
            Self::NativeValidators => "native-validators",
        }
    }

    #[must_use]
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Alint => "Alint",
            Self::CargoDeny => "Cargo Deny",
            Self::CargoMachete => "Cargo Machete",
            Self::Jactionlint => "Jactionlint",
            Self::Markdownlint => "Markdownlint",
            Self::StrictJson => "Strict JSON",
            Self::FrontmatterId => "Frontmatter ID",
            Self::LinkCheck => "Link Check",
            Self::NativeValidators => "Native Validators",
        }
    }

    #[must_use]
    pub fn all() -> [Self; 9] {
        [
            Self::Alint,
            Self::CargoDeny,
            Self::CargoMachete,
            Self::Jactionlint,
            Self::Markdownlint,
            Self::StrictJson,
            Self::FrontmatterId,
            Self::LinkCheck,
            Self::NativeValidators,
        ]
    }

    #[must_use]
    pub fn repository_validators() -> [Self; 3] {
        [Self::Alint, Self::CargoDeny, Self::CargoMachete]
    }
}

impl ValidatorKind {
    #[must_use]
    pub fn consumer_verify() -> [Self; 6] {
        [
            Self::Alint,
            Self::Markdownlint,
            Self::StrictJson,
            Self::FrontmatterId,
            Self::LinkCheck,
            Self::NativeValidators,
        ]
    }

    #[must_use]
    pub fn from_verify_name(name: &str) -> Option<Self> {
        Self::consumer_verify()
            .into_iter()
            .find(|kind| kind.job_id() == name)
    }
}

pub const CRATE_JOB_ID_PREFIX: &str = "rust-";

pub const TOFU_JOB_ID_PREFIX: &str = "tofu-";

pub const TOFU_DISPLAY_PREFIX: &str = "OpenToFu — ";

#[must_use]
pub fn is_crate_job_id(id: &str) -> bool {
    id.starts_with(CRATE_JOB_ID_PREFIX) || id.starts_with(TOFU_JOB_ID_PREFIX)
}

pub fn validate_job_id(id: &str) -> Result<(), ContractError> {
    if id.is_empty()
        || !id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_'))
    {
        return Err(ContractError::identity(
            "job.id",
            format!("bad_job_id:{id}"),
        ));
    }
    if !is_crate_job_id(id) && id.contains("velnor") {
        return Err(ContractError::identity(
            "job.id",
            format!("branded_job_id:{id}"),
        ));
    }
    Ok(())
}

#[must_use]
pub fn slugify_segment(name: &str) -> String {
    let mut slug = String::new();
    for byte in name.bytes() {
        let lower = byte.to_ascii_lowercase();
        let push = if lower.is_ascii_alphanumeric() {
            Some(lower)
        } else if slug.bytes().last().is_some_and(|b| b != b'-') && !slug.is_empty() {
            Some(b'-')
        } else {
            None
        };
        if let Some(byte) = push {
            slug.push(char::from(byte));
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

#[must_use]
pub fn is_safe_display_name(name: &str) -> bool {
    !name.contains("${{") && !name.chars().any(char::is_control)
}

fn sanitize_display_text(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    for ch in label.chars() {
        if ch.is_control() {
            out.push('?');
        } else {
            out.push(ch);
        }
    }
    out.replace("${{", "$?{{")
}

#[must_use]
pub fn crate_display_label(package_name: &str, manifest: &str) -> String {
    if !package_name.trim().is_empty() {
        return sanitize_display_text(package_name);
    }
    let parent = manifest.rsplit('/').nth(1).unwrap_or_default();
    if parent.is_empty() || parent == "." {
        "workspace".to_owned()
    } else {
        sanitize_display_text(parent)
    }
}

#[must_use]
pub fn crate_display_name(package_name: &str, manifest: &str, configuration: &str) -> String {
    let label = crate_display_label(package_name, manifest);
    let composed = if configuration == "default" {
        format!("Rust / {label}")
    } else {
        format!("Rust / {label} ({configuration})")
    };
    sanitize_display_text(&composed)
}

#[must_use]
pub fn tofu_display_name(root_label: &str) -> String {
    sanitize_display_text(&format!("{TOFU_DISPLAY_PREFIX}{root_label}"))
}

#[must_use]
pub fn assign_crate_job_ids(
    crates: &BTreeSet<(String, String, String)>,
    prefix: &str,
) -> BTreeMap<(String, String), String> {
    let mut assigned = BTreeMap::new();
    let mut taken = BTreeSet::new();
    for (package_id, package_name, configuration) in crates {
        let slug = slugify_segment(package_name);
        let mut base = if slug.is_empty() {
            format!("{prefix}workspace")
        } else {
            format!("{prefix}{slug}")
        };
        if configuration != "default" {
            base.push('-');
            base.push_str(&slugify_segment(configuration));
        }
        let mut id = base;
        if taken.contains(&id) {
            let digest = digest_b3(format!("{package_id}\0{configuration}").as_bytes());
            let short = digest.get(..8).unwrap_or(&digest);
            id = format!("{id}-{short}");
        }
        taken.insert(id.clone());
        assigned.insert((package_id.clone(), configuration.clone()), id);
    }
    assigned
}
