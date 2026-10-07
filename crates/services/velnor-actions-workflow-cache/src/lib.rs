//! Cache step templates: MBX objects, tools, Mise, Tofu, seeds.
//!
//! Restore/save emission plus writer election over validated action
//! refs: native MBX object-cache actions with Rust preflight, the
//! tools-cache key namespace, qualified Mise caches (built-in, shared
//! sources, Cargo-only fallback), Tofu provider saves, and the
//! read-only tool seed with writer election per key.

pub mod cache_elect;
pub mod cache_p08;
mod cache_p08_detect;
pub mod cache_steps;
pub mod mbx_gc_policy;
pub mod tofu_cache;
pub mod tool_seed;
mod tool_seed_admission;
mod tool_seed_test_support;
