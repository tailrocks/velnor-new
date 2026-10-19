use super::super::*;

#[test]
fn root_scope_is_twelve_hex_of_the_root_digest() {
    assert_eq!(root_scope(""), "re3b0c44298fc");
    let nested = root_scope("crates/velnor-runner");
    assert_eq!(nested.len(), 13);
    assert!(nested.starts_with('r'));
    assert_ne!(nested, root_scope(""));
    assert_eq!(
        trusted_root_prefix(CacheFamily::Rsdeps, ""),
        "velnor-v2-rsdeps-trusted-re3b0c44298fc-"
    );
    assert_eq!(
        trusted_root_prefix(CacheFamily::RsdepsHelper, ""),
        "velnor-v2-rsdeps-helper-trusted-re3b0c44298fc-"
    );
}

#[test]
fn lane_profile_dirs_come_from_the_family_specs() {
    assert_eq!(SeedLane::Build.profile_dir(""), "target/debug");
    assert_eq!(
        SeedLane::Clippy.profile_dir("crates/runner"),
        "crates/runner/target/velnor-clippy/debug"
    );
    assert_eq!(SeedLane::Doc.profile_dir(""), "target/velnor-doc/debug");
    assert_eq!(SeedLane::Helper.profile_dir(""), "target/release");
    assert_eq!(SeedLane::for_task_kind("clippy"), Some(SeedLane::Clippy));
    assert_eq!(SeedLane::for_task_kind("nextest"), None);
    assert_eq!(SeedLane::for_task_kind("helper"), None);
}

#[test]
fn rsdeps_outputs_are_plan_cache_outputs_with_bounded_ordinals() {
    for name in ["rsdeps_key_0", "rsdeps_restore_keys_15", "rsdeps_exact_3"] {
        assert!(is_plan_cache_output(name), "{name}");
    }
    for name in [
        "rsdeps_key_",
        "rsdeps_key_16",
        "rsdeps_exact_x",
        "rsdeps_helper_key",
    ] {
        assert!(!is_plan_cache_output(name), "{name}");
    }
    assert_eq!(rsdeps_output_ordinal("rsdeps_restore_keys_7"), Some(7));
    assert_eq!(
        plan_output_expr(&rsdeps_key_output(2)),
        "${{needs.plan.outputs.rsdeps_key_2}}"
    );
    assert_eq!(
        key_step_expr(&rsdeps_restore_keys_output(0)),
        "${{steps.cache-keys.outputs.rsdeps_restore_keys_0}}"
    );
    assert!(
        cache_output_reserve_utf16_bytes(0, MAX_RSDEPS_ROOTS)
            <= PLAN_CACHE_OUTPUTS_RESERVE_UTF16_BYTES
    );
    assert!(cache_output_reserve_utf16_bytes(0, 2) > cache_output_reserve_utf16_bytes(0, 0));
}

#[test]
fn save_conditions_name_every_lane_and_never_run_on_cancellation() {
    for lane in SeedLane::RSDEPS {
        assert!(
            RSDEPS_SAVE_CONDITION.contains(&format!(
                "steps.seed-{}.outcome == 'success'",
                lane.as_str()
            )),
            "{lane:?}"
        );
    }
    for condition in [RSDEPS_SAVE_CONDITION, RSDEPS_HELPER_SAVE_CONDITION] {
        assert!(condition.starts_with("always() && !cancelled() && "));
        assert!(condition.contains("steps.seed-trim.outcome == 'success'"));
    }
    assert!(RSDEPS_HELPER_SAVE_CONDITION.contains("steps.seed-helper.outcome == 'success'"));
}
