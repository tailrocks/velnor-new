//! Tool-seed action payload and name-independent setup admission cases.

use super::*;
use velnor_actions_contract::StepRole;

#[test]
fn typed_prelude_call_stays_one_step_and_conditional_checkout_stays_cold() {
    let setup = setup_config();
    let mut rendered_job = job(vec![
        crate::steps::checkout_step(CHECKOUT).expect("checkout"),
        mise_shell(),
    ]);
    crate::cache_p08::ensure_tools_cache_v2(
        "fixture",
        &mut rendered_job,
        &setup,
        false,
        TARGET,
        CHECKOUT,
    )
    .expect("setup and seed");
    let prelude_index = rendered_job
        .steps
        .iter()
        .position(|step| step.role == Some(StepRole::ToolsCacheIdentity))
        .expect("V2 prelude");
    rendered_job.steps[prelude_index].name = "Restore Velnor tool seed".to_owned();
    assert!(
        crate::cache_p08::ensure_tools_cache_v2(
            "renamed-seed",
            &mut rendered_job,
            &setup,
            false,
            TARGET,
            CHECKOUT,
        )
        .is_err()
    );
    assert_eq!(
        rendered_job.steps[prelude_index].name,
        "Restore Velnor tool seed"
    );

    let mut conditional = job(vec![
        {
            let mut step = crate::steps::checkout_step(CHECKOUT).expect("checkout");
            step.condition = Some("always()".to_owned());
            step
        },
        mise_shell(),
    ]);
    crate::cache_p08::ensure_tools_cache_v2(
        "conditional-checkout",
        &mut conditional,
        &setup,
        false,
        TARGET,
        CHECKOUT,
    )
    .expect("cold setup");
    assert!(
        conditional
            .steps
            .iter()
            .all(|step| step.role != Some(StepRole::ToolsCacheIdentity))
    );
}

#[test]
fn action_composite_contains_the_guarded_fixed_root_and_exact_input() {
    let file = action_file("0.1.0").expect("action");
    assert_eq!(file.path, TOOL_SEED_ACTION_PATH);
    assert!(file.bytes.contains("/opt/velnor/seed"), "{}", file.bytes);
    assert!(file.bytes.contains("$SEED_KEY"), "{}", file.bytes);
    assert!(file.bytes.contains("inputs.cache_key"), "{}", file.bytes);
    assert!(
        file.bytes.contains("trusted_seed_is_trusted"),
        "{}",
        file.bytes
    );
    assert!(file.bytes.contains("seed_admitted:"), "{}", file.bytes);
    assert!(file.bytes.contains("id: copy"), "{}", file.bytes);
    assert!(
        file.bytes
            .contains("value: ${{ steps.copy.outputs.seed_admitted }}"),
        "{}",
        file.bytes
    );
    assert!(file.bytes.contains("seed_admitted=true"), "{}", file.bytes);
    assert!(
        !file.bytes.contains("seed_admitted=false"),
        "{}",
        file.bytes
    );
    assert!(file.bytes.contains("unset "), "{}", file.bytes);
    assert!(!file.bytes.contains("rm "), "{}", file.bytes);
}

#[test]
fn seed_action_tree_exports_admission_from_the_copy_step() {
    use crate::yaml::Yaml;

    fn field<'a>(node: &'a Yaml, key: &str) -> &'a Yaml {
        let Yaml::Map(entries) = node else {
            panic!("expected mapping containing {key}");
        };
        entries
            .iter()
            .find_map(|(name, value)| (name == key).then_some(value))
            .unwrap_or_else(|| panic!("missing mapping field {key}"))
    }

    let action = super::action_yaml(
        "Copy matching tool seed",
        &std::collections::BTreeMap::new(),
        "printf done",
    );
    let output = field(field(&action, "outputs"), "seed_admitted");
    assert_eq!(
        field(output, "value"),
        &Yaml::Str("${{ steps.copy.outputs.seed_admitted }}".to_owned())
    );
    let Yaml::Seq(steps) = field(field(&action, "runs"), "steps") else {
        panic!("composite steps are a sequence");
    };
    assert_eq!(field(&steps[0], "id"), &Yaml::Str("copy".to_owned()));
}
