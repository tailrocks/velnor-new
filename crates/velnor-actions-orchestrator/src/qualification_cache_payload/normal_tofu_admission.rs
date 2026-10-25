//! Check normal `ToFu` restore outputs against runner context and backend state.

#[path = "normal_tofu_admission_backend.rs"]
mod backend;
#[path = "normal_tofu_admission_context.rs"]
mod context;
#[path = "normal_tofu_admission_preflight.rs"]
mod preflight;

pub use backend::{
    NormalTofuProvidersCacheBackendEvidence, observe_normal_tofu_providers_cache_backend,
};
pub use preflight::{
    NormalTofuProvidersCacheRestorePreflight, preflight_normal_tofu_providers_cache_restore,
};

#[cfg(test)]
#[path = "tests/normal_tofu_admission_tests.rs"]
mod tests;
