//! Typed family selection for the composed product-release coordinator.
//!
//! Rust package releases use [`crate::release_spec`] and retain their own
//! lifecycle. This type describes the independently published Velnor product
//! families that a Schema 2 coordinator may compose.

use velnor_actions_workflow_steps::RenderError;

/// One product family with its stable coordinator identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProductReleaseFamily {
    /// Linux runner and `DinD` image assets.
    Images,
    /// The macOS Velnor host binary.
    Binary,
    /// Linux and macOS Velnor Actions binaries.
    Generator,
}

impl ProductReleaseFamily {
    /// Stable human-readable label used by coordinator jobs.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Images => "runner images",
            Self::Binary => "velnor-host binary",
            Self::Generator => "velnor-actions generator",
        }
    }

    /// Reusable workflow path for this family.
    #[must_use]
    pub const fn workflow_path(self) -> &'static str {
        match self {
            Self::Images => ".github/workflows/product-release-images.yml",
            Self::Binary => ".github/workflows/product-release-binary.yml",
            Self::Generator => ".github/workflows/product-release-generator.yml",
        }
    }

    /// Coordinator job that reconciles the family release before delegation.
    #[must_use]
    pub const fn prepare_job_id(self) -> &'static str {
        match self {
            Self::Images => "prepare-images",
            Self::Binary => "prepare-binary",
            Self::Generator => "prepare-generator",
        }
    }

    /// Coordinator job that delegates to the family reusable workflow.
    #[must_use]
    pub const fn call_job_id(self) -> &'static str {
        match self {
            Self::Images => "release-images",
            Self::Binary => "release-binary",
            Self::Generator => "release-generator",
        }
    }
}

/// Non-empty, duplicate-free family selection in canonical emission order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductReleaseSpec {
    families: Vec<ProductReleaseFamily>,
}

impl ProductReleaseSpec {
    /// Construct a coordinator selection from requested product families.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty or duplicate selection. Families are
    /// sorted into their stable order so request order cannot affect output.
    pub fn new(
        families: impl IntoIterator<Item = ProductReleaseFamily>,
    ) -> Result<Self, RenderError> {
        let mut selected = Vec::new();
        for family in families {
            if selected.contains(&family) {
                return Err(RenderError::InvalidWorkflow(format!(
                    "product_release_family_duplicate:{}",
                    family.as_str()
                )));
            }
            selected.push(family);
        }
        if selected.is_empty() {
            return Err(RenderError::InvalidWorkflow(
                "product_release_families_empty".to_owned(),
            ));
        }
        selected.sort_unstable();
        Ok(Self { families: selected })
    }

    /// Selected families in canonical emission order.
    #[must_use]
    pub fn families(&self) -> &[ProductReleaseFamily] {
        &self.families
    }

    /// Whether the family is included in this coordinator invocation.
    #[must_use]
    pub fn includes(&self, family: ProductReleaseFamily) -> bool {
        self.families.binary_search(&family).is_ok()
    }
}

impl ProductReleaseFamily {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Images => "images",
            Self::Binary => "binary",
            Self::Generator => "generator",
        }
    }
}

#[cfg(test)]
#[path = "product_release/tests.rs"]
mod tests;
