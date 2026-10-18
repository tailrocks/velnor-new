//! Check normal `ToFu` restore outputs against runner context and backend state.

#[path = "normal_tofu_admission_backend.rs"]
mod backend;
#[path = "normal_tofu_admission_context.rs"]
mod context;

pub use backend::{
    NormalTofuProvidersCacheBackendEvidence, observe_normal_tofu_providers_cache_backend,
};

#[cfg(test)]
#[path = "tests/normal_tofu_admission_tests.rs"]
mod tests;
