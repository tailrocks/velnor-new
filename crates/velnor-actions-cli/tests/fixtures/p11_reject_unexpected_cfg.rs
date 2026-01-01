//! Negative fixture: unknown `cfg` name (rejected by
//! `unexpected_cfgs = "deny"` once `--check-cfg` is active).

/// Violating gate: the condition name matches no declared `--check-cfg`.
#[cfg(p11_never_a_real_cfg)]
pub fn gated() -> u32 {
    1
}

/// Fallback compiled when the unknown condition is absent.
#[cfg(not(p11_never_a_real_cfg))]
pub fn gated() -> u32 {
    0
}
