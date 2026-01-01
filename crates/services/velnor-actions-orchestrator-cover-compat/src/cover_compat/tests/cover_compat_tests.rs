use super::*;

#[test]
fn compat_is_deterministic_digest() {
    let obligations = vec![
        obligation(
            "stack/rust/a/clippy/default",
            &digest_b3(b"t1"),
            &digest_b3(b"i1"),
        ),
        obligation(
            "stack/rust/b/clippy/default",
            &digest_b3(b"t2"),
            &digest_b3(b"i2"),
        ),
    ];
    let plan = plan_with("ubuntu-26.04", obligations);
    let first = baseline_compat_for_plan(&plan).expect("compat");
    let second = baseline_compat_for_plan(&plan).expect("compat");
    assert_eq!(first, second);
    assert!(validate_digest(&first).is_ok());
}

#[test]
fn compat_ignores_obligation_order() {
    let left = plan_with(
        "ubuntu-26.04",
        vec![
            obligation(
                "stack/rust/a/clippy/default",
                &digest_b3(b"t1"),
                &digest_b3(b"i1"),
            ),
            obligation(
                "stack/rust/b/clippy/default",
                &digest_b3(b"t2"),
                &digest_b3(b"i2"),
            ),
        ],
    );
    let right = plan_with(
        "ubuntu-26.04",
        vec![
            obligation(
                "stack/rust/b/clippy/default",
                &digest_b3(b"t2"),
                &digest_b3(b"i2"),
            ),
            obligation(
                "stack/rust/a/clippy/default",
                &digest_b3(b"t1"),
                &digest_b3(b"i1"),
            ),
        ],
    );
    assert_eq!(
        baseline_compat_for_plan(&left).expect("compat"),
        baseline_compat_for_plan(&right).expect("compat")
    );
}

#[test]
fn compat_survives_source_edits_but_not_toolchain_or_shape() {
    let base = plan_with(
        "ubuntu-26.04",
        vec![obligation(
            "stack/rust/a/clippy/default",
            &digest_b3(b"task"),
            &digest_b3(b"input-v1"),
        )],
    );
    let edited = plan_with(
        "ubuntu-26.04",
        vec![obligation(
            "stack/rust/a/clippy/default",
            &digest_b3(b"task"),
            &digest_b3(b"input-v2"),
        )],
    );
    assert_eq!(
        baseline_compat_for_plan(&base).expect("compat"),
        baseline_compat_for_plan(&edited).expect("compat"),
        "source content never invalidates the execution shape"
    );
    let retooled = plan_with(
        "ubuntu-26.04",
        vec![obligation(
            "stack/rust/a/clippy/default",
            &digest_b3(b"task-v2"),
            &digest_b3(b"input-v1"),
        )],
    );
    assert_ne!(
        baseline_compat_for_plan(&base).expect("compat"),
        baseline_compat_for_plan(&retooled).expect("compat"),
        "task digest drift must invalidate"
    );
    let relabeled = plan_with(
        "ubuntu-24.04",
        vec![obligation(
            "stack/rust/a/clippy/default",
            &digest_b3(b"task"),
            &digest_b3(b"input-v1"),
        )],
    );
    assert_ne!(
        baseline_compat_for_plan(&base).expect("compat"),
        baseline_compat_for_plan(&relabeled).expect("compat"),
        "runner label drift must invalidate"
    );
    let grown = plan_with(
        "ubuntu-26.04",
        vec![
            obligation(
                "stack/rust/a/clippy/default",
                &digest_b3(b"task"),
                &digest_b3(b"input-v1"),
            ),
            obligation(
                "stack/rust/b/clippy/default",
                &digest_b3(b"task"),
                &digest_b3(b"input-v1"),
            ),
        ],
    );
    assert_ne!(
        baseline_compat_for_plan(&base).expect("compat"),
        baseline_compat_for_plan(&grown).expect("compat"),
        "task set drift must invalidate"
    );
}

#[test]
fn compat_defined_for_empty_plans() {
    let plan = plan_with("ubuntu-26.04", Vec::new());
    let compat = baseline_compat_for_plan(&plan).expect("compat");
    assert!(validate_digest(&compat).is_ok());
}

#[test]
fn numeric_id_is_deterministic_nonzero_and_name_bound() {
    let base = "a".repeat(40);
    let compat = digest_b3(b"compat");
    let name = artifact_id_for_baseline(&base, &compat).expect("name");
    let first = baseline_artifact_numeric_id(&name);
    assert_eq!(first, baseline_artifact_numeric_id(&name));
    assert!(first > 0);
    let other = artifact_id_for_baseline(&base, &digest_b3(b"other")).expect("name");
    assert_ne!(
        first,
        baseline_artifact_numeric_id(&other),
        "distinct names must fingerprint distinctly"
    );
}
