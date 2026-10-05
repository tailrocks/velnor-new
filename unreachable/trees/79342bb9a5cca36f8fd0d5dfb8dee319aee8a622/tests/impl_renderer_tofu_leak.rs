//! F4 leak test: renderer tofu owns representation only, never domain decisions.
//!
//! Contract F4 (decision-based): renderer/transport/CLI product code must
//! not decide tofu argv/flags, root selection, or lock/version/scope
//! semantics. The tofu renderer surface (step templates, writer
//! election, layer arms) is representation plus boundary validation,
//! so none of the domain-decision symbols below may appear in its
//! sources. Scans compile-time-embedded sources (`include_str!`), so
//! the pin travels with the tree it guards.

/// Tofu-owned renderer sources (templates + election + layer arms).
const SOURCES: [(&str, &str); 3] = [
    ("tofu_cache.rs", include_str!("../src/tofu_cache.rs")),
    ("cache_elect.rs", include_str!("../src/cache_elect.rs")),
    ("cache_steps.rs", include_str!("../src/cache_steps.rs")),
];

/// Domain-decision symbols that must never leak into renderer sources.
const FORBIDDEN: [&str; 14] = [
    "tofu_payload_argv",
    "-lockfile=",
    "-backend=false",
    "-input=false",
    "select_roots",
    "qualify_roots",
    "key_for_root",
    "root_for_key",
    "inspect_lockfile",
    "provider_hash",
    "admits_version",
    "required_version",
    "family_of",
    "fmt_scope",
];

/// No tofu domain-decision symbol appears in renderer tofu sources.
#[test]
fn renderer_tofu_sources_decide_no_domain_semantics() {
    for (file, body) in SOURCES {
        assert!(!body.is_empty(), "{file} embedded");
        for token in FORBIDDEN {
            assert!(
                !body.contains(token),
                "{file} leaks domain decision symbol {token}"
            );
        }
    }
}
