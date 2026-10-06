//! Tofu closure cases, second file (size split): extras, digests,
//! modules, and varfiles.
use velnor_actions_contract::Provenance;
use velnor_actions_tofu::closure::resolve_closure_at_root;
use velnor_actions_tofu::file_cache::FileCache;

use crate::impl_tofu_closure::{proposal, seed, seed_modules};
use crate::support::{Outcome, TempDir};

#[test]
fn declared_extras_and_digests_bind() -> Outcome {
    let root = TempDir::create("tofu-closure-declared")?;
    seed(&root, "", false)?;
    root.write("policy.rego", "package x\n")?;
    let mut task = proposal("validate", "");
    task.identity.declared_inputs = vec!["policy.rego".to_owned(), "absent.txt".to_owned()];
    let closure = resolve_closure_at_root(
        root.path(),
        &task,
        "graph",
        "tool",
        "plat",
        &mut FileCache::new(),
    )?;
    assert!(matches!(
        closure.inputs.get("declared_extra:0:policy.rego"),
        Some(Provenance::Known { .. })
    ));
    assert!(matches!(
        closure.inputs.get("declared_extra:1:absent.txt"),
        Some(Provenance::AbsentProven { .. })
    ));
    for (name, want) in [
        ("local_deps", "graph"),
        ("toolchain", "tool"),
        ("platform", "plat"),
    ] {
        assert_eq!(
            closure.inputs.get(name),
            Some(&Provenance::Known {
                digest: want.to_owned()
            }),
            "{name}"
        );
    }
    Ok(())
}

#[test]
fn undeclared_reads_mark_vcs_unknown() -> Outcome {
    let root = TempDir::create("tofu-closure-vcs")?;
    seed(&root, "", false)?;
    let mut task = proposal("validate", "");
    task.identity.undeclared_reads = true;
    let closure =
        resolve_closure_at_root(root.path(), &task, "g", "t", "p", &mut FileCache::new())?;
    assert!(closure.unknown_inputs().contains(&"vcs"));
    Ok(())
}

#[test]
fn init_and_validate_share_the_load_set() -> Outcome {
    let root = TempDir::create("tofu-closure-init")?;
    seed(&root, "", false)?;
    let init = resolve_closure_at_root(
        root.path(),
        &proposal("init", ""),
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    let validate = resolve_closure_at_root(
        root.path(),
        &proposal("validate", ""),
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    assert_eq!(
        init.inputs.get("source_tree"),
        validate.inputs.get("source_tree")
    );
    assert!(init.unknown_inputs().is_empty());
    Ok(())
}

#[test]
fn modules_input_binds_local_closure() -> Outcome {
    let root = TempDir::create("tofu-closure-modules")?;
    seed_modules(&root, "")?;
    let closure = resolve_closure_at_root(
        root.path(),
        &proposal("validate", ""),
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    assert!(matches!(
        closure.inputs.get("modules"),
        Some(Provenance::Known { .. })
    ));
    assert!(closure.unknown_inputs().is_empty());
    Ok(())
}

#[test]
fn module_edit_flips_modules_digest() -> Outcome {
    let root = TempDir::create("tofu-closure-mod-edit")?;
    seed_modules(&root, "")?;
    let before = resolve_closure_at_root(
        root.path(),
        &proposal("validate", ""),
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    root.write("mods/a/main.tf", "variable \"x\" {}\nvariable \"y\" {}\n")?;
    let after = resolve_closure_at_root(
        root.path(),
        &proposal("validate", ""),
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    assert_ne!(before.inputs.get("modules"), after.inputs.get("modules"));
    Ok(())
}

#[test]
fn dynamic_source_marks_modules_unknown() -> Outcome {
    let root = TempDir::create("tofu-closure-dynamic")?;
    root.write("main.tf", "module \"d\" {\n  source = var.x\n}\n")?;
    let closure = resolve_closure_at_root(
        root.path(),
        &proposal("validate", ""),
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    assert!(closure.unknown_inputs().contains(&"modules"));
    Ok(())
}

#[test]
fn missing_target_marks_modules_unknown() -> Outcome {
    let root = TempDir::create("tofu-closure-mod-missing")?;
    root.write("main.tf", "module \"a\" {\n  source = \"./absent\"\n}\n")?;
    let closure = resolve_closure_at_root(
        root.path(),
        &proposal("validate", ""),
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    assert!(closure.unknown_inputs().contains(&"modules"));
    Ok(())
}

#[test]
fn varfiles_input_binds_auto_tfvars() -> Outcome {
    let root = TempDir::create("tofu-closure-varfiles")?;
    seed(&root, "", false)?;
    root.write("terraform.tfvars", "x = 1\n")?;
    root.write("extra.auto.tfvars", "y = 2\n")?;
    let before = resolve_closure_at_root(
        root.path(),
        &proposal("validate", ""),
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    assert!(matches!(
        before.inputs.get("varfiles"),
        Some(Provenance::Known { .. })
    ));
    root.write("extra.tfvars", "x = 9\n")?;
    let same = resolve_closure_at_root(
        root.path(),
        &proposal("validate", ""),
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    assert_eq!(before.inputs.get("varfiles"), same.inputs.get("varfiles"));
    root.write("extra.auto.tfvars", "y = 3\n")?;
    let after = resolve_closure_at_root(
        root.path(),
        &proposal("validate", ""),
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    assert_ne!(before.inputs.get("varfiles"), after.inputs.get("varfiles"));
    Ok(())
}

#[test]
fn force_added_varfiles_still_bound() -> Outcome {
    let root = TempDir::create("tofu-closure-forced")?;
    root.write(".gitignore", "*.tfvars\n")?;
    root.write("main.tf", "variable \"x\" {}\n")?;
    root.write("forced.auto.tfvars", "x = 1\n")?;
    let closure = resolve_closure_at_root(
        root.path(),
        &proposal("validate", ""),
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    assert!(matches!(
        closure.inputs.get("varfiles"),
        Some(Provenance::Known { .. })
    ));
    Ok(())
}

#[test]
fn fmt_excludes_modules_and_varfiles() -> Outcome {
    let root = TempDir::create("tofu-closure-fmt-mod")?;
    seed_modules(&root, "")?;
    root.write("terraform.tfvars", "x = 1\n")?;
    let closure = resolve_closure_at_root(
        root.path(),
        &proposal("fmt", ""),
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    assert!(matches!(
        closure.inputs.get("modules"),
        Some(Provenance::AbsentProven { .. })
    ));
    assert!(matches!(
        closure.inputs.get("varfiles"),
        Some(Provenance::AbsentProven { .. })
    ));
    assert!(closure.unknown_inputs().is_empty());
    Ok(())
}
