//! Freshness input classes and runner-image evidence (ver §2, §4).
//!
//! Freshness requirements apply per input class; the hosted-image record
//! carries `ImageOS`/`ImageVersion` evidence so family changes qualify as
//! new platform identities. Labels never fix package sets: evidence
//! records exactly the observed image, nothing more.

use serde::{Deserialize, Serialize};

use crate::errors::ContractError;

/// The nine freshness input classes (ver §2).
pub const FRESHNESS_CLASSES: [&str; 9] = [
    "compiler",
    "bootstrap",
    "tools",
    "crates",
    "velnor",
    "actions",
    "alint",
    "runner",
    "deferred",
];

/// One per-class freshness requirement (ver §2).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FreshnessRequirement {
    /// Input class from [`FRESHNESS_CLASSES`].
    pub class: String,
    /// Maximum pin age in days for this class.
    pub max_age_days: u32,
}

/// Observed hosted-runner image evidence (ver §4).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerImageEvidence {
    /// Runner `ImageOS` value; `unknown` when unobserved.
    pub image_os: String,
    /// Runner `ImageVersion` value; `unknown` when unobserved.
    pub image_version: String,
}

/// Marker for image fields the generator could not observe.
///
/// The generator never sees the provisioned runner, so label text is
/// never split into these fields: observed facts arrive only through
/// [`RunnerImageEvidence::observed`], everything else is unobserved.
pub const UNOBSERVED_IMAGE_VALUE: &str = "unknown";

impl FreshnessRequirement {
    /// Validate class membership and a positive age bound.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        validate_freshness_class(&self.class)?;
        if self.max_age_days < 1 {
            return Err(ContractError::config(
                file,
                format!("freshness.{}.max_age_days", self.class),
                "must_be_at_least_one",
            ));
        }
        Ok(())
    }
}

impl RunnerImageEvidence {
    /// Record genuinely observed provisioner facts.
    ///
    /// Both values must be semantic and neither may be the
    /// [`UNOBSERVED_IMAGE_VALUE`] marker, so observed and unobserved
    /// records are disjoint by construction (P03-4).
    /// # Errors
    pub fn observed(os: &str, version: &str) -> Result<Self, ContractError> {
        for (field, value) in [("image_os", os), ("image_version", version)] {
            crate::cachekey::validate_semantic_text(field, value)?;
            if value == UNOBSERVED_IMAGE_VALUE {
                return Err(ContractError::identity(field, "unobserved_marker"));
            }
        }
        Ok(Self {
            image_os: os.to_owned(),
            image_version: version.to_owned(),
        })
    }

    /// Record explicitly unobserved image evidence (generation time).
    #[must_use]
    pub fn unobserved() -> Self {
        Self {
            image_os: UNOBSERVED_IMAGE_VALUE.to_owned(),
            image_version: UNOBSERVED_IMAGE_VALUE.to_owned(),
        }
    }

    /// True when both fields carry the unobserved marker.
    #[must_use]
    pub fn is_unobserved(&self) -> bool {
        self.image_os == UNOBSERVED_IMAGE_VALUE && self.image_version == UNOBSERVED_IMAGE_VALUE
    }

    /// Validate observed image values are present and semantic.
    /// # Errors
    pub fn validate(&self) -> Result<(), ContractError> {
        for (field, value) in [
            ("image_os", self.image_os.as_str()),
            ("image_version", self.image_version.as_str()),
        ] {
            crate::cachekey::validate_semantic_text(field, value)?;
        }
        Ok(())
    }
}

/// Validate one input class against the closed set.
/// # Errors
pub fn validate_freshness_class(class: &str) -> Result<(), ContractError> {
    if FRESHNESS_CLASSES.contains(&class) {
        Ok(())
    } else {
        Err(ContractError::identity(
            "freshness.class",
            format!("unknown_class:{class}"),
        ))
    }
}

/// True when two runner labels belong to different image families.
///
/// The `-arm` suffix selects a variant within one family; any other
/// label difference is a family change that MUST requalify.
#[must_use]
pub fn runner_family_changed(old_label: &str, new_label: &str) -> bool {
    runner_family(old_label) != runner_family(new_label)
}

/// The image family of a runner label (variant suffix stripped).
fn runner_family(label: &str) -> &str {
    label.strip_suffix("-arm").unwrap_or(label)
}
