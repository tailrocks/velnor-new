//! Tofu closure + task-kind cases.
use velnor_actions_contract::{ProposedTask, Provenance};
use velnor_actions_tofu::closure::resolve_closure_at_root;
use velnor_actions_tofu::file_cache::FileCache;
use velnor_actions_tofu::kinds::TofuTaskKind;

use crate::support::{Outcome, TempDir};

/// Minimal tofu proposal for `kind` in `unit`.
pub(crate) fn proposal(kind: &str, unit: &str) -> Result<ProposedTask, Box<dyn std::error::Error>> {
    let parsed = TofuTaskKind::parse(kind).unwrap_or(TofuTaskKind::Validate);
    let group = velnor_actions_tofu::TofuTaskGroup {
        root: unit.to_owned(),
        kind: parsed,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let mut task = velnor_actions_tofu::propose_task(&group)?;
    if parsed.as_str() != kind {
        kind.clone_into(&mut task.task_kind);
    }
    Ok(task)
}

/// Seed a unit root with config, vars, and an optional lockfile.
pub(crate) fn seed(
    root: &TempDir,
    unit: &str,
    lock: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let prefix = if unit.is_empty() {
        String::new()
    } else {
        format!("{unit}/")
    };
    root.write(&format!("{prefix}main.tf"), "variable \"x\" {}\n")?;
    root.write(&format!("{prefix}extra.tfvars"), "x = 1\n")?;
    if lock {
        root.write(&format!("{prefix}.terraform.lock.hcl"), "# lock\n")?;
    }
    Ok(())
}

/// Seed a unit root with a local module edge.
pub(crate) fn seed_modules(root: &TempDir, unit: &str) -> Result<(), Box<dyn std::error::Error>> {
    let prefix = if unit.is_empty() {
        String::new()
    } else {
        format!("{unit}/")
    };
    root.write(
        &format!("{prefix}main.tf"),
        "module \"a\" {\n  source = \"./mods/a\"\n}\n",
    )?;
    root.write(&format!("{prefix}mods/a/main.tf"), "variable \"x\" {}\n")?;
    Ok(())
}

#[test]
fn kinds_round_trip_and_reject_unknown() {
    for (token, kind) in [
        ("fmt", TofuTaskKind::Fmt),
        ("init", TofuTaskKind::InitForValidate),
        ("validate", TofuTaskKind::Validate),
    ] {
        assert_eq!(TofuTaskKind::parse(token).expect("known"), kind);
        assert_eq!(kind.as_str(), token);
    }
    assert!(TofuTaskKind::parse("plan").is_err());
    assert!(TofuTaskKind::parse("").is_err());
    assert!(TofuTaskKind::parse("Fmt").is_err());
}

#[test]
fn unknown_kind_fails_closed() -> Outcome {
    let root = TempDir::create("tofu-closure-kind")?;
    let err = resolve_closure_at_root(
        root.path(),
        &proposal("plan", "")?,
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )
    .expect_err("unknown kind");
    assert!(err.to_string().contains("unknown_kind"), "{err}");
    Ok(())
}

#[test]
fn validate_closure_binds_effective_set_and_lock() -> Outcome {
    let root = TempDir::create("tofu-closure-validate")?;
    seed(&root, "", true)?;
    let closure = resolve_closure_at_root(
        root.path(),
        &proposal("validate", "")?,
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    assert_eq!(closure.unknown_inputs(), [] as [&str; 0]);
    assert!(matches!(
        closure.inputs.get("source_tree"),
        Some(Provenance::Known { .. })
    ));
    assert!(matches!(
        closure.inputs.get("lockfile"),
        Some(Provenance::Known { .. })
    ));
    Ok(())
}

#[test]
fn source_edit_flips_the_closure_digest() -> Outcome {
    let root = TempDir::create("tofu-closure-edit")?;
    seed(&root, "", false)?;
    let before = resolve_closure_at_root(
        root.path(),
        &proposal("validate", "")?,
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    root.write("main.tf", "variable \"x\" {}\nvariable \"y\" {}\n")?;
    let after = resolve_closure_at_root(
        root.path(),
        &proposal("validate", "")?,
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    assert_ne!(
        before.inputs.get("source_tree"),
        after.inputs.get("source_tree")
    );
    Ok(())
}

#[test]
fn tfvars_edits_move_fmt_but_not_validate() -> Outcome {
    let root = TempDir::create("tofu-closure-fmt")?;
    seed(&root, "", false)?;
    let fmt_before = resolve_closure_at_root(
        root.path(),
        &proposal("fmt", "")?,
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    let val_before = resolve_closure_at_root(
        root.path(),
        &proposal("validate", "")?,
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    root.write("extra.tfvars", "x = 2\n")?;
    let fmt_after = resolve_closure_at_root(
        root.path(),
        &proposal("fmt", "")?,
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    let val_after = resolve_closure_at_root(
        root.path(),
        &proposal("validate", "")?,
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    assert_ne!(
        fmt_before.inputs.get("source_tree"),
        fmt_after.inputs.get("source_tree")
    );
    assert_eq!(
        val_before.inputs.get("source_tree"),
        val_after.inputs.get("source_tree")
    );
    Ok(())
}

#[test]
fn fmt_ignores_the_lockfile() -> Outcome {
    let root = TempDir::create("tofu-closure-fmt-lock")?;
    seed(&root, "", true)?;
    let closure = resolve_closure_at_root(
        root.path(),
        &proposal("fmt", "")?,
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    assert!(matches!(
        closure.inputs.get("lockfile"),
        Some(Provenance::AbsentProven { .. })
    ));
    Ok(())
}

#[test]
fn missing_lockfile_is_proven_absent_for_validate() -> Outcome {
    let root = TempDir::create("tofu-closure-no-lock")?;
    seed(&root, "", false)?;
    let closure = resolve_closure_at_root(
        root.path(),
        &proposal("validate", "")?,
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    assert!(matches!(
        closure.inputs.get("lockfile"),
        Some(Provenance::AbsentProven { .. })
    ));
    assert_eq!(closure.unknown_inputs(), [] as [&str; 0]);
    Ok(())
}

#[test]
fn json_only_root_has_no_fmt_inputs() -> Outcome {
    let root = TempDir::create("tofu-closure-json")?;
    root.write("main.tf.json", "{\"variable\": {\"x\": {}}}")?;
    let closure = resolve_closure_at_root(
        root.path(),
        &proposal("fmt", "")?,
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    assert!(matches!(
        closure.inputs.get("source_tree"),
        Some(Provenance::AbsentProven { .. })
    ));
    Ok(())
}

#[test]
fn subdir_unit_scopes_to_itself() -> Outcome {
    let root = TempDir::create("tofu-closure-sub")?;
    seed(&root, "infra", false)?;
    root.write("main.tf", "variable \"other\" {}\n")?;
    let closure = resolve_closure_at_root(
        root.path(),
        &proposal("validate", "infra")?,
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    assert_eq!(closure.unknown_inputs(), [] as [&str; 0]);
    root.write("main.tf", "variable \"changed\" {}\n")?;
    let again = resolve_closure_at_root(
        root.path(),
        &proposal("validate", "infra")?,
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    assert_eq!(closure.inputs, again.inputs);
    Ok(())
}

#[test]
#[cfg(unix)]
fn symlink_in_unit_is_unknown() -> Outcome {
    let root = TempDir::create("tofu-closure-link")?;
    seed(&root, "", false)?;
    std::os::unix::fs::symlink(root.path().join("main.tf"), root.path().join("linked.tf"))?;
    let closure = resolve_closure_at_root(
        root.path(),
        &proposal("validate", "")?,
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    assert!(closure.unknown_inputs().contains(&"source_tree"));
    Ok(())
}

#[test]
fn hidden_dirs_never_enter_the_tree() -> Outcome {
    let root = TempDir::create("tofu-closure-hidden")?;
    seed(&root, "", false)?;
    root.write(".terraform/modules/x/main.tf", "variable \"cached\" {}\n")?;
    let before = resolve_closure_at_root(
        root.path(),
        &proposal("validate", "")?,
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    root.write(".terraform/modules/x/main.tf", "variable \"changed\" {}\n")?;
    let after = resolve_closure_at_root(
        root.path(),
        &proposal("validate", "")?,
        "g",
        "t",
        "p",
        &mut FileCache::new(),
    )?;
    assert_eq!(before.inputs, after.inputs);
    Ok(())
}
