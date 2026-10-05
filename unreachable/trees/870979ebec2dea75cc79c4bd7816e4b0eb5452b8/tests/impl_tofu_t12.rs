//! T12 tofu selection cases: calling-root narrowing end to end.
//!
//! Each case builds a two-root git repo (`stacks/a` calls `./mods/m`,
//! `stacks/b` stands alone, plus the fixture root crate), commits
//! base and head, and plans the pull request: affected roots carry
//! `affected_by_change` on every triple leg while unaffected roots
//! and stacks do not.

use std::fs;
use std::path::Path;

use velnor_actions_contract::Plan;

use crate::impl_common::{TestResult, make_repo};
use crate::impl_select::{commit, plan_pr, reasons_for};

/// Fixture config with two sorted tofu roots beside the root crate.
fn tofu_config() -> String {
    "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n[stacks.tofu]\nroots = [\"stacks/a\", \"stacks/b\"]\n"
        .to_owned()
}

/// Write the two-root tofu tree at `root`.
fn write_roots(root: &Path) -> TestResult {
    for dir in ["stacks/a/mods/m", "stacks/b"] {
        fs::create_dir_all(root.join(dir))?;
    }
    fs::write(
        root.join("stacks/a/main.tf"),
        "module \"m\" {\n  source = \"./mods/m\"\n}\n",
    )?;
    fs::write(root.join("stacks/a/mods/m/main.tf"), "variable \"x\" {}\n")?;
    fs::write(root.join("stacks/b/main.tf"), "variable \"y\" {}\n")?;
    Ok(())
}

/// Reasons for every obligation whose task ID contains `member`.
fn reasons(plan: &Plan, member: &str) -> Vec<String> {
    reasons_for(plan, member)
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// Assert every triple leg of `root` changed and `other` did not.
fn assert_narrow(plan: &Plan, root: &str, other: &str) {
    let hit = reasons(plan, root);
    assert_eq!(hit.len(), 3, "{root} proposes the triple: {hit:?}");
    assert!(
        hit.iter().all(|reason| reason == "affected_by_change"),
        "{root}: {hit:?}"
    );
    let miss = reasons(plan, other);
    assert_eq!(miss.len(), 3, "{other} proposes the triple: {miss:?}");
    assert!(
        miss.iter().all(|reason| reason != "affected_by_change"),
        "{other}: {miss:?}"
    );
}

#[test]
fn module_target_change_selects_calling_root_only() -> TestResult {
    let repo = make_repo(&tofu_config())?;
    let root = repo.path();
    write_roots(root)?;
    let base = commit(root, "base")?;
    fs::write(
        root.join("stacks/a/mods/m/main.tf"),
        "variable \"x\" {}\nvariable \"z\" {}\n",
    )?;
    let head = commit(root, "head")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    assert_narrow(&plan, "stacks/a", "stacks/b");
    for reason in reasons(&plan, "stack/rust") {
        assert_ne!(reason, "affected_by_change", "rust stays unselected");
    }
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.contains("tofu_select_all")),
        "{warnings:?}"
    );
    Ok(())
}

#[test]
fn removed_base_edge_still_selects_its_caller() -> TestResult {
    let repo = make_repo(&tofu_config())?;
    let root = repo.path();
    write_roots(root)?;
    let base = commit(root, "base")?;
    fs::write(root.join("stacks/a/main.tf"), "variable \"solo\" {}\n")?;
    fs::write(
        root.join("stacks/a/mods/m/main.tf"),
        "variable \"x\" {}\nvariable \"z\" {}\n",
    )?;
    let head = commit(root, "head")?;
    let (plan, _) = plan_pr(root, Some(&base), &head)?;
    assert_narrow(&plan, "stacks/a", "stacks/b");
    Ok(())
}

#[test]
fn dynamic_base_source_widens_with_recorded_warning() -> TestResult {
    let repo = make_repo(&tofu_config())?;
    let root = repo.path();
    write_roots(root)?;
    fs::write(
        root.join("stacks/a/main.tf"),
        "module \"m\" {\n  source = \"./mods/m\"\n}\nmodule \"d\" {\n  source = var.dynamic\n}\n",
    )?;
    let base = commit(root, "base")?;
    fs::write(
        root.join("stacks/a/main.tf"),
        "module \"m\" {\n  source = \"./mods/m\"\n}\n",
    )?;
    fs::write(
        root.join("stacks/b/main.tf"),
        "variable \"y\" {}\nvariable \"w\" {}\n",
    )?;
    let head = commit(root, "head")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    for root in ["stacks/a", "stacks/b"] {
        let hit = reasons(&plan, root);
        assert!(
            hit.iter().all(|reason| reason == "affected_by_change"),
            "{root}: {hit:?}"
        );
    }
    assert!(
        warnings
            .iter()
            .any(|warning| warning.starts_with("tofu_select_all:dynamic_source:")),
        "{warnings:?}"
    );
    Ok(())
}

#[test]
fn rust_change_leaves_tofu_unaffected() -> TestResult {
    let repo = make_repo(&tofu_config())?;
    let root = repo.path();
    write_roots(root)?;
    let base = commit(root, "base")?;
    fs::write(root.join("src/lib.rs"), "pub fn f() {}\npub fn g() {}\n")?;
    let head = commit(root, "head")?;
    let (plan, warnings) = plan_pr(root, Some(&base), &head)?;
    let rust = reasons(&plan, "stack/rust");
    assert!(
        rust.iter().all(|reason| reason == "affected_by_change"),
        "rust selects: {rust:?}"
    );
    for root in ["stacks/a", "stacks/b"] {
        let miss = reasons(&plan, root);
        assert_eq!(miss.len(), 3, "{root} proposes the triple");
        assert!(
            miss.iter().all(|reason| reason != "affected_by_change"),
            "{root}: {miss:?}"
        );
    }
    assert!(
        !warnings
            .iter()
            .any(|warning| warning.contains("tofu_select_all")),
        "{warnings:?}"
    );
    Ok(())
}
