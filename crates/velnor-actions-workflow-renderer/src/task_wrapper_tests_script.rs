//! Generated frame-check script shape (`ShellCheck`-clean by construction).

use super::VERSION;

#[test]
fn reserved_frame_keys_render_each_case_pattern_once() {
    let patterns = super::super::script::reserved_frame_env_patterns();
    let mut unique = patterns.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), patterns.len(), "{patterns:?}");
    for credential in crate::toolchain_env::STEP_CREDENTIAL_DENYLIST {
        assert!(patterns.contains(&credential), "{credential}");
    }
    for name in ["VELNOR_TASK_ID", "*_TOKEN", "GH_CONFIG_DIR", "CHECKPOINT_*"] {
        assert!(patterns.contains(&name), "{name}");
    }
    let script = super::super::script::task_script(VERSION);
    let arm = format!("    {}) fail_frame ;;", patterns.join("|"));
    assert_eq!(script.matches(&arm).count(), 1, "{script}");
}

#[test]
fn compound_frame_checks_group_before_the_failure_branch() {
    // `A && B || fail` reads as if-then-else but is not (SC2015); the
    // grouped `{ A && B; } || fail` keeps the same semantics without it.
    let script = super::super::script::task_script(VERSION);
    for line in script.lines() {
        if line.contains("&&") && line.contains("|| fail_frame") {
            assert!(line.contains("; } || fail_frame"), "{line}");
        }
    }
    assert!(script.contains(
        "{ [[ \"$frame_bytes\" =~ ^[0-9]{1,7}$ ]] && (( frame_bytes <= 1048576 )); } || fail_frame"
    ));
}
