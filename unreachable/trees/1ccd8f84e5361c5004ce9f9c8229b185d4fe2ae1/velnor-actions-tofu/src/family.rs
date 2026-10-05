//! Tofu file families: config, override, test, var, lock, other.
//!
//! Families classify by file NAME only (never content): the config
//! family reuses the S1 `config_shape`
//! suffixes, override is the config subset whose stem is `override` or
//! ends with `_override`, test/var/lock match their own suffixes, and
//! everything else is [`Family::Other`]. `.tofuvars` is deliberately
//! NOT a var spelling (S2 excludes it from the fmt set; `OpenTofu` does
//! not load it), and `.tfvars.json` IS (JSON var files load as vars
//! even though JSON never formats).

use crate::effective::config_shape;

/// Lockfile name: validate selection only, never the fmt set.
pub const LOCKFILE_NAME: &str = ".terraform.lock.hcl";

/// File family of one tofu file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    /// `.tf`/`.tofu`/`.tf.json`/`.tofu.json` load-set member.
    Config,
    /// Config file whose stem selects override merge order.
    Override,
    /// `.tftest.hcl`/`.tofutest.hcl`: fmt only, never the load set.
    Test,
    /// `.tfvars`/`.tfvars.json`: fmt only, never the load set.
    Var,
    /// `.terraform.lock.hcl`: validate selection only.
    Lock,
    /// Anything else (docs, examples, JSON configs excepted above).
    Other,
}

/// Family of one base file name (no directories).
#[must_use]
pub fn family_of(file_name: &str) -> Family {
    if file_name == LOCKFILE_NAME {
        return Family::Lock;
    }
    if let Some(shape) = config_shape(file_name) {
        return if is_override_stem(&shape.stem) {
            Family::Override
        } else {
            Family::Config
        };
    }
    if has_stem(file_name, ".tftest.hcl") || has_stem(file_name, ".tofutest.hcl") {
        return Family::Test;
    }
    if has_stem(file_name, ".tfvars") || has_stem(file_name, ".tfvars.json") {
        return Family::Var;
    }
    Family::Other
}

/// True when `stem` selects override merge order.
#[must_use]
pub fn is_override_stem(stem: &str) -> bool {
    stem == "override" || stem.ends_with("_override")
}

/// True when `file_name` is an auto-loaded var file.
///
/// `terraform.tfvars` plus `*.auto.tfvars`, with the `.json`
/// counterparts (`terraform.tfvars.json`, `*.auto.tfvars.json`);
/// the `*` must be non-empty. H2 enumeration (T11) builds on this.
#[must_use]
pub fn is_auto_var(file_name: &str) -> bool {
    if file_name == "terraform.tfvars" || file_name == "terraform.tfvars.json" {
        return true;
    }
    has_stem(file_name, ".auto.tfvars") || has_stem(file_name, ".auto.tfvars.json")
}

/// True when `file_name` ends with `suffix` over a non-empty stem.
fn has_stem(file_name: &str, suffix: &str) -> bool {
    file_name
        .strip_suffix(suffix)
        .is_some_and(|stem| !stem.is_empty())
}
