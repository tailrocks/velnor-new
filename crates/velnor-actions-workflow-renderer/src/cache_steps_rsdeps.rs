//! Dependency seed (`rsdeps`) cache steps over the typed family specs.
//!
//! Every lookup, restore, and save of a dependency seed takes its `path`
//! input from [`FamilySpec::path_input`] for one validated workspace root,
//! so the Plan lookup, crate-job restores, and the Cache seed restore and
//! save archive byte-identical path lists.
//!
//! [`FamilySpec::path_input`]: velnor_actions_contract::workflow::cache_family::FamilySpec::path_input

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::cache_family::{CacheFamily, root_prefix};
use velnor_actions_contract::{Step, StepRole};

use crate::RenderError;
use crate::steps::{action_step, validate_uses};

/// How one dependency seed cache step touches the family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeedCacheUse {
    /// `actions/cache/restore` with `lookup-only: true` (exact-key probe).
    Lookup,
    /// Read-only restore with a restore prefix.
    Restore,
    /// The Cache seed's producer-gated save.
    Save,
}

/// True for a dependency seed family.
#[must_use]
fn is_seed_family(family: CacheFamily) -> bool {
    matches!(family, CacheFamily::Rsdeps | CacheFamily::RsdepsHelper)
}

/// Reject a workspace root that cannot be spliced into a cache path.
/// # Errors
///
/// Returns [`RenderError::BadCommand`] for absolute, dotted, or odd roots.
pub fn validate_seed_root(root: &str) -> Result<(), RenderError> {
    let ok = root.is_empty()
        || (!root.starts_with('/')
            && !root.ends_with('/')
            && root.split('/').all(|segment| {
                !segment.is_empty()
                    && segment != "."
                    && segment != ".."
                    && segment
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
            }));
    if ok {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!("bad_seed_root:{root}")))
    }
}

/// The family's one `path:` input for `root`.
/// # Errors
///
/// Returns [`RenderError::BadCommand`] for a non-seed family or a bad root.
pub fn seed_path_input(family: CacheFamily, root: &str) -> Result<String, RenderError> {
    if !is_seed_family(family) {
        return Err(RenderError::BadCommand("not_a_seed_family".to_owned()));
    }
    validate_seed_root(root)?;
    Ok(family.spec().path_input(Some(&root_prefix(root))))
}

/// One dependency seed lookup, restore, or save step.
///
/// `restore_keys` is required for [`SeedCacheUse::Restore`] and rejected
/// otherwise.
/// # Errors
///
/// Returns [`RenderError`] for a bad pin, family, root, key, or prefix use.
pub fn seed_cache_step(
    uses: &str,
    family: CacheFamily,
    root: &str,
    cache_use: SeedCacheUse,
    key: &str,
    restore_keys: Option<&str>,
) -> Result<Step, RenderError> {
    validate_uses(uses)?;
    let action = match cache_use {
        SeedCacheUse::Save => "actions/cache/save@",
        SeedCacheUse::Lookup | SeedCacheUse::Restore => "actions/cache/restore@",
    };
    if !uses.starts_with(action) {
        return Err(RenderError::BadActionRef(format!("bad_cache_uses:{uses}")));
    }
    if key.trim().is_empty() || key.contains([' ', '\n']) {
        return Err(RenderError::BadCommand("bad_cache_key".to_owned()));
    }
    let mut with = BTreeMap::from([
        ("key".to_owned(), key.to_owned()),
        ("path".to_owned(), seed_path_input(family, root)?),
    ]);
    match (cache_use, restore_keys) {
        (SeedCacheUse::Restore, Some(prefix)) if !prefix.trim().is_empty() => {
            with.insert("restore-keys".to_owned(), prefix.to_owned());
        }
        (SeedCacheUse::Lookup, None) => {
            with.insert("lookup-only".to_owned(), "true".to_owned());
        }
        (SeedCacheUse::Save, None) => {}
        _ => return Err(RenderError::BadCommand("bad_seed_restore_keys".to_owned())),
    }
    let mut step = action_step(&step_name(family, root, cache_use), uses, with)?;
    step.role = Some(match cache_use {
        SeedCacheUse::Lookup => StepRole::DependencySeedLookup,
        SeedCacheUse::Restore => StepRole::DependencySeedRestore,
        SeedCacheUse::Save => StepRole::DependencySeedSave,
    });
    Ok(step)
}

fn step_name(family: CacheFamily, root: &str, cache_use: SeedCacheUse) -> String {
    let verb = match cache_use {
        SeedCacheUse::Lookup => "Look up",
        SeedCacheUse::Restore => "Restore",
        SeedCacheUse::Save => "Save",
    };
    let what = if family == CacheFamily::RsdepsHelper {
        "helper dependency seed"
    } else {
        "dependency seed"
    };
    if root.is_empty() {
        format!("{verb} {what}")
    } else {
        format!("{verb} {what} ({root})")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use velnor_actions_contract::StepKind;

    const RESTORE: &str = "actions/cache/restore@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";
    const SAVE: &str = "actions/cache/save@55cc8345863c7cc4c66a329aec7e433d2d1c52a9";

    fn with(step: &Step) -> &BTreeMap<String, String> {
        let StepKind::Action { with, .. } = &step.kind else {
            panic!("seed cache steps are actions");
        };
        with
    }

    #[test]
    fn lookup_restore_and_save_share_one_path_list() {
        let key = "${{steps.cache-keys.outputs.rsdeps_key_1}}";
        let lookup = seed_cache_step(
            RESTORE,
            CacheFamily::Rsdeps,
            "nested/ws",
            SeedCacheUse::Lookup,
            key,
            None,
        )
        .expect("lookup");
        let restore = seed_cache_step(
            RESTORE,
            CacheFamily::Rsdeps,
            "nested/ws",
            SeedCacheUse::Restore,
            key,
            Some("${{steps.cache-keys.outputs.rsdeps_restore_keys_1}}"),
        )
        .expect("restore");
        let save = seed_cache_step(
            SAVE,
            CacheFamily::Rsdeps,
            "nested/ws",
            SeedCacheUse::Save,
            key,
            None,
        )
        .expect("save");
        let paths = "nested/ws/target/debug\nnested/ws/target/velnor-clippy/debug\nnested/ws/target/velnor-doc/debug";
        for step in [&lookup, &restore, &save] {
            assert_eq!(with(step)["path"], paths, "{}", step.name);
        }
        assert_eq!(with(&lookup)["lookup-only"], "true");
        assert!(!with(&restore).contains_key("lookup-only"));
        assert!(!with(&save).contains_key("restore-keys"));
        assert_eq!(lookup.role, Some(StepRole::DependencySeedLookup));
        assert_eq!(restore.name, "Restore dependency seed (nested/ws)");
        assert_eq!(save.role, Some(StepRole::DependencySeedSave));
    }

    #[test]
    fn bad_roots_families_and_prefix_uses_are_rejected() {
        let key = "k";
        for root in ["/abs", "a/", "a/../b", "a b"] {
            assert!(
                seed_path_input(CacheFamily::Rsdeps, root).is_err(),
                "{root}"
            );
        }
        assert!(seed_path_input(CacheFamily::Sources, "").is_err());
        assert!(
            seed_cache_step(
                RESTORE,
                CacheFamily::Rsdeps,
                "",
                SeedCacheUse::Restore,
                key,
                None
            )
            .is_err()
        );
        assert!(
            seed_cache_step(
                RESTORE,
                CacheFamily::Rsdeps,
                "",
                SeedCacheUse::Lookup,
                key,
                Some("p")
            )
            .is_err()
        );
        assert!(
            seed_cache_step(
                RESTORE,
                CacheFamily::Rsdeps,
                "",
                SeedCacheUse::Save,
                key,
                None
            )
            .is_err()
        );
        let helper = seed_cache_step(
            SAVE,
            CacheFamily::RsdepsHelper,
            "",
            SeedCacheUse::Save,
            key,
            None,
        )
        .expect("helper save");
        assert_eq!(with(&helper)["path"], "target/release");
        assert_eq!(helper.name, "Save helper dependency seed");
    }
}
