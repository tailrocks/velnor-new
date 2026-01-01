//! Tofu provider-input toolchain entries: the per-root provider
//! surface declaration bound into the toolchain identity.
use velnor_actions_contract::ContractError;
use velnor_actions_tofu::{key_for_root, provider_toolchain_entries};

fn entry_for(unit: &str, kind: &str, tofu: &str) -> Result<String, ContractError> {
    let entries = provider_toolchain_entries(&key_for_root(unit), kind, tofu)?;
    entries
        .first()
        .cloned()
        .ok_or_else(|| ContractError::identity("entries", "missing_entry"))
}

#[test]
fn provider_entries_are_single_sorted_valid_digests() -> Result<(), ContractError> {
    let entries = provider_toolchain_entries(&key_for_root(""), "init", "opentofu@1.13.1")?;
    assert_eq!(entries.len(), 1, "one surface entry per task");
    let mut sorted = entries.clone();
    sorted.sort();
    assert_eq!(entries, sorted, "components stay sorted");
    let entry = &entries[0];
    assert!(entry.starts_with("tofu-provider-inputs:b3-"), "{entry}");
    let digest = entry.strip_prefix("tofu-provider-inputs:").expect("prefix");
    assert!(velnor_actions_contract::is_valid_digest(digest), "{entry}");
    Ok(())
}

#[test]
fn provider_entries_flip_on_root_kind_slot_and_tofu_pin() -> Result<(), ContractError> {
    let base = entry_for("root", "init", "opentofu@1.13.1")?;
    assert_eq!(base, entry_for("root", "init", "opentofu@1.13.1")?);
    assert_ne!(base, entry_for("stacks/vpc", "init", "opentofu@1.13.1")?);
    assert_eq!(
        base,
        entry_for("root", "validate", "opentofu@1.13.1")?,
        "init and validate read the same providers"
    );
    assert_ne!(
        base,
        entry_for("root", "fmt", "opentofu@1.13.1")?,
        "fmt binds the excluded slot"
    );
    assert_ne!(base, entry_for("root", "init", "opentofu@1.13.2")?);
    assert_ne!(
        base,
        entry_for("", "init", "opentofu@1.13.1")?,
        "repo root and literal root directory have distinct identities"
    );
    Ok(())
}

#[test]
fn toolchain_inputs_carry_exact_pin_plus_surface() -> Result<(), ContractError> {
    use velnor_actions_tofu::{
        TofuTaskGroup, TofuTaskKind, propose_task, toolchain_inputs_for_task,
    };
    let group = TofuTaskGroup {
        root: String::new(),
        kind: TofuTaskKind::Validate,
        configuration: "default".to_owned(),
        no_targets: false,
    };
    let task = propose_task(&group)?;
    let inputs = toolchain_inputs_for_task(&task, vec!["opentofu@1.13.1".to_owned()])?;
    assert_eq!(inputs.tools, vec!["opentofu@1.13.1".to_owned()]);
    assert_eq!(inputs.components.len(), 1);
    assert!(
        inputs.components[0].starts_with("tofu-provider-inputs:b3-"),
        "{}",
        inputs.components[0]
    );
    assert_eq!(inputs.compile_driver, "tofu");
    assert_eq!(inputs.test_runner, "none");
    let mut bogus = task.clone();
    bogus.task_kind = "bogus".to_owned();
    assert!(
        toolchain_inputs_for_task(&bogus, vec!["opentofu@1.13.1".to_owned()]).is_err(),
        "unknown kinds fail closed"
    );
    Ok(())
}

#[test]
fn provider_entries_reject_unknown_kinds() {
    for bad in ["", "bogus", "INIT", "fmt\n", "init --upgrade"] {
        assert!(
            provider_toolchain_entries(&key_for_root(""), bad, "opentofu@1.13.1").is_err(),
            "{bad:?} must fail closed"
        );
    }
    assert!(provider_toolchain_entries("root", "init", "opentofu@1.13.1").is_err());
}
