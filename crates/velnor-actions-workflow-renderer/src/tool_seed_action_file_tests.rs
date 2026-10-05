//! Tool-seed action payload and name-independent setup admission cases.

use super::*;

#[test]
fn local_seed_action_payload_and_order_are_not_name_authorized() {
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
    let seed_index = rendered_job
        .steps
        .iter()
        .position(is_tool_seed_action)
        .expect("seed action");
    rendered_job.steps[seed_index].name = "Checkout".to_owned();
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
    assert_eq!(rendered_job.steps[seed_index].name, "Checkout");

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
            .all(|step| !is_tool_seed_action(step))
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
    assert!(file.bytes.contains("unset "), "{}", file.bytes);
    assert!(!file.bytes.contains("rm "), "{}", file.bytes);
}
