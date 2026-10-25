//! Runtime identity evidence and exact cache key binding.

mod abi;
mod image;
mod tools;
mod types;
mod validation;

pub use types::{
    BoundQualificationCacheKeys, QualificationCacheRestoreExpectation,
    QualificationRuntimeAbiObservation, QualificationRuntimeIdentity,
    QualificationRuntimeIdentityField, QualificationRuntimeIdentityRequirements,
    QualificationRuntimeImageDigests, QualificationRuntimeImageIdentity,
    QualificationRuntimePlatform, QualificationRuntimeScaleSet,
    QualificationRuntimeToolObservation,
};
pub(crate) use validation::validate_runtime_identity;
