//! Tofu closure + task-kind cases.
use std::collections::BTreeMap;
use std::ffi::OsString;

use velnor_actions_contract::{
    CachePolicy, IdentityInputs, ProposedTask, Provenance, ResourceClass, ResourceDemand,
};
use velnor_actions_tofu::argv::tofu_payload_argv;
use velnor_actions_tofu::closure::resolve_closure_at_root;
use velnor_actions_tofu::kinds::TofuTaskKind;

use crate::support::{Outcome, TempDir};

/// Minimal tofu proposal for `kind` in `unit`.
fn proposal(kind: &str, unit: &str) -> ProposedTask {
    ProposedTask {
        task_id: format!("stack/tofu/root/{kind}/default"),
        stack_id: "tofu".to_owned(),
        component_id: format!("tofu:{unit}"),
        task_kind: kind.to_owned(),
        configuration: "default".to_owned(),
        depends_on: Vec::new(),
        gated_by: Vec::new(),
        reads: Vec::new(),
        writes: Vec::new(),
        outputs: Vec::new(),
        resource: ResourceDemand {
            class: ResourceClass::Compiler,
            cpu_milli: None,
            memory_mb: None,
            needs_network: false,
            service: None,
        },
        cache_policy: CachePolicy {
            allow_compilation_reuse: false,
            allow_task_reuse: false,
        },
        identity: IdentityInputs {
            unit_id: String::new(),
            unit_key: "root".to_owned(),
            unit_path: unit.to_owned(),
            project_root: ".".to_owned(),
            target: "host".to_owned(),
            features: Vec::new(),
            flags: Vec::new(),
            compile_driver: "tofu".to_owned(),
            test_runner: "tofu".to_owned(),
            environment: BTreeMap::new(),
            declared_inputs: Vec::new(),
            undeclared_reads: false,
        },
        payload: match TofuTaskKind::parse(kind) {
            Ok(parsed) => {
                tofu_payload_argv(parsed, unit).unwrap_or_else(|_| vec![OsString::from("tofu")])
            }
            Err(_) => vec![OsString::from("tofu")],
        },
        display_name: String::new(),
        uses_clock: false,
        uses_random: false,
        no_targets: false,
        runner_profile: "default".to_owned(),
    }
}

/// Seed a unit root with config, vars, and an optional lockfile.
fn seed(root: &TempDir, unit: &str, lock: bool) -> Result<(), Box<dyn std::error::Error>> {
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
fn seed_modules(root: &TempDir, unit: &str) -> Result<(), Box<dyn std::error::Error>> {
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
    let err = resolve_closure_at_root(root.path(), &proposal("plan", ""), "g", "t", "p")
        .expect_err("unknown kind");
    assert!(err.to_string().contains("unknown_kind"), "{err}");
    Ok(())
}

#[test]
fn validate_closure_binds_effective_set_and_lock() -> Outcome {
    let root = TempDir::create("tofu-closure-validate")?;
    seed(&root, "", true)?;
    let closure = resolve_closure_at_root(root.path(), &proposal("validate", ""), "g", "t", "p")?;
    assert!(closure.unknown_inputs().is_empty());
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
    let before = resolve_closure_at_root(root.path(), &proposal("validate", ""), "g", "t", "p")?;
    root.write("main.tf", "variable \"x\" {}\nvariable \"y\" {}\n")?;
    let after = resolve_closure_at_root(root.path(), &proposal("validate", ""), "g", "t", "p")?;
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
    let fmt_before = resolve_closure_at_root(root.path(), &proposal("fmt", ""), "g", "t", "p")?;
    let val_before =
        resolve_closure_at_root(root.path(), &proposal("validate", ""), "g", "t", "p")?;
    root.write("extra.tfvars", "x = 2\n")?;
    let fmt_after = resolve_closure_at_root(root.path(), &proposal("fmt", ""), "g", "t", "p")?;
    let val_after = resolve_closure_at_root(root.path(), &proposal("validate", ""), "g", "t", "p")?;
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
    let closure = resolve_closure_at_root(root.path(), &proposal("fmt", ""), "g", "t", "p")?;
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
    let closure = resolve_closure_at_root(root.path(), &proposal("validate", ""), "g", "t", "p")?;
    assert!(matches!(
        closure.inputs.get("lockfile"),
        Some(Provenance::AbsentProven { .. })
    ));
    assert!(closure.unknown_inputs().is_empty());
    Ok(())
}

#[test]
fn json_only_root_has_no_fmt_inputs() -> Outcome {
    let root = TempDir::create("tofu-closure-json")?;
    root.write("main.tf.json", "{\"variable\": {\"x\": {}}}")?;
    let closure = resolve_closure_at_root(root.path(), &proposal("fmt", ""), "g", "t", "p")?;
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
    let closure =
        resolve_closure_at_root(root.path(), &proposal("validate", "infra"), "g", "t", "p")?;
    assert!(closure.unknown_inputs().is_empty());
    root.write("main.tf", "variable \"changed\" {}\n")?;
    let again =
        resolve_closure_at_root(root.path(), &proposal("validate", "infra"), "g", "t", "p")?;
    assert_eq!(closure.inputs, again.inputs);
    Ok(())
}

#[test]
#[cfg(unix)]
fn symlink_in_unit_is_unknown() -> Outcome {
    let root = TempDir::create("tofu-closure-link")?;
    seed(&root, "", false)?;
    std::os::unix::fs::symlink(root.path().join("main.tf"), root.path().join("linked.tf"))?;
    let closure = resolve_closure_at_root(root.path(), &proposal("validate", ""), "g", "t", "p")?;
    assert!(closure.unknown_inputs().contains(&"source_tree"));
    Ok(())
}

#[test]
fn hidden_dirs_never_enter_the_tree() -> Outcome {
    let root = TempDir::create("tofu-closure-hidden")?;
    seed(&root, "", false)?;
    root.write(".terraform/modules/x/main.tf", "variable \"cached\" {}\n")?;
    let before = resolve_closure_at_root(root.path(), &proposal("validate", ""), "g", "t", "p")?;
    root.write(".terraform/modules/x/main.tf", "variable \"changed\" {}\n")?;
    let after = resolve_closure_at_root(root.path(), &proposal("validate", ""), "g", "t", "p")?;
    assert_eq!(before.inputs, after.inputs);
    Ok(())
}

#[test]
fn declared_extras_and_digests_bind() -> Outcome {
    let root = TempDir::create("tofu-closure-declared")?;
    seed(&root, "", false)?;
    root.write("policy.rego", "package x\n")?;
    let mut task = proposal("validate", "");
    task.identity.declared_inputs = vec!["policy.rego".to_owned(), "absent.txt".to_owned()];
    let closure = resolve_closure_at_root(root.path(), &task, "graph", "tool", "plat")?;
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
    let closure = resolve_closure_at_root(root.path(), &task, "g", "t", "p")?;
    assert!(closure.unknown_inputs().contains(&"vcs"));
    Ok(())
}

#[test]
fn init_and_validate_share_the_load_set() -> Outcome {
    let root = TempDir::create("tofu-closure-init")?;
    seed(&root, "", false)?;
    let init = resolve_closure_at_root(root.path(), &proposal("init", ""), "g", "t", "p")?;
    let validate = resolve_closure_at_root(root.path(), &proposal("validate", ""), "g", "t", "p")?;
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
    let closure = resolve_closure_at_root(root.path(), &proposal("validate", ""), "g", "t", "p")?;
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
    let before = resolve_closure_at_root(root.path(), &proposal("validate", ""), "g", "t", "p")?;
    root.write("mods/a/main.tf", "variable \"x\" {}\nvariable \"y\" {}\n")?;
    let after = resolve_closure_at_root(root.path(), &proposal("validate", ""), "g", "t", "p")?;
    assert_ne!(before.inputs.get("modules"), after.inputs.get("modules"));
    Ok(())
}

#[test]
fn dynamic_source_marks_modules_unknown() -> Outcome {
    let root = TempDir::create("tofu-closure-dynamic")?;
    root.write("main.tf", "module \"d\" {\n  source = var.x\n}\n")?;
    let closure = resolve_closure_at_root(root.path(), &proposal("validate", ""), "g", "t", "p")?;
    assert!(closure.unknown_inputs().contains(&"modules"));
    Ok(())
}

#[test]
fn missing_target_marks_modules_unknown() -> Outcome {
    let root = TempDir::create("tofu-closure-mod-missing")?;
    root.write("main.tf", "module \"a\" {\n  source = \"./absent\"\n}\n")?;
    let closure = resolve_closure_at_root(root.path(), &proposal("validate", ""), "g", "t", "p")?;
    assert!(closure.unknown_inputs().contains(&"modules"));
    Ok(())
}

#[test]
fn varfiles_input_binds_auto_tfvars() -> Outcome {
    let root = TempDir::create("tofu-closure-varfiles")?;
    seed(&root, "", false)?;
    root.write("terraform.tfvars", "x = 1\n")?;
    root.write("extra.auto.tfvars", "y = 2\n")?;
    let before = resolve_closure_at_root(root.path(), &proposal("validate", ""), "g", "t", "p")?;
    assert!(matches!(
        before.inputs.get("varfiles"),
        Some(Provenance::Known { .. })
    ));
    root.write("extra.tfvars", "x = 9\n")?;
    let same = resolve_closure_at_root(root.path(), &proposal("validate", ""), "g", "t", "p")?;
    assert_eq!(before.inputs.get("varfiles"), same.inputs.get("varfiles"));
    root.write("extra.auto.tfvars", "y = 3\n")?;
    let after = resolve_closure_at_root(root.path(), &proposal("validate", ""), "g", "t", "p")?;
    assert_ne!(before.inputs.get("varfiles"), after.inputs.get("varfiles"));
    Ok(())
}

#[test]
fn force_added_varfiles_still_bound() -> Outcome {
    let root = TempDir::create("tofu-closure-forced")?;
    root.write(".gitignore", "*.tfvars\n")?;
    root.write("main.tf", "variable \"x\" {}\n")?;
    root.write("forced.auto.tfvars", "x = 1\n")?;
    let closure = resolve_closure_at_root(root.path(), &proposal("validate", ""), "g", "t", "p")?;
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
    let closure = resolve_closure_at_root(root.path(), &proposal("fmt", ""), "g", "t", "p")?;
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
