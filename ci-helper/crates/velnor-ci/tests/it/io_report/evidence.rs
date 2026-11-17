// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

macro_rules! invalid_restore {
    ($name:ident, $steps:expr) => {
        #[test]
        fn $name() -> ResultTest {
            let repo = Repo::new("report-invalid-restore");
            let plan = json!([{"family":"target","root":".","restore_step":"restore","save_step":"save"}]);
            let metrics = report(&repo, &plan, $steps)?;
            assert!(!metrics.verification_errors.is_empty(), "{metrics:?}");
            assert!(velnor_ci::report::check(&[metrics]).is_err());
            Ok(())
        }
    };
}
invalid_restore!(null_steps_are_unknown, "null");
invalid_restore!(
    contradictory_exact_hit_is_unknown,
    r#"{"restore":{"outcome":"success","outputs":{"cache-primary-key":"key","cache-matched-key":"other","cache-hit":"true"}}}"#
);
invalid_restore!(array_steps_are_unknown, "[]");
invalid_restore!(scalar_steps_are_unknown, "false");
invalid_restore!(absent_restore_is_unknown, "{}");
invalid_restore!(
    absent_primary_key_is_unknown,
    r#"{"restore":{"outcome":"success","outputs":{}}}"#
);
invalid_restore!(
    illtyped_outputs_are_unknown,
    r#"{"restore":{"outcome":"success","outputs":[]}}"#
);
invalid_restore!(
    failed_restore_is_unknown,
    r#"{"restore":{"outcome":"failure","outputs":{"cache-primary-key":"key"}}}"#
);
invalid_restore!(
    illtyped_primary_key_is_unknown,
    r#"{"restore":{"outcome":"success","outputs":{"cache-primary-key":1}}}"#
);
invalid_restore!(
    illtyped_matched_key_is_unknown,
    r#"{"restore":{"outcome":"success","outputs":{"cache-primary-key":"key","cache-matched-key":false}}}"#
);
invalid_restore!(
    invalid_hit_is_unknown,
    r#"{"restore":{"outcome":"success","outputs":{"cache-primary-key":"key","cache-hit":"maybe"}}}"#
);

#[test]
fn skipped_restore_and_successful_miss_are_verified_cold() -> ResultTest {
    let repo = Repo::new("report-known-cold");
    let plan = json!([{"family":"target","root":".","restore_step":"restore","save_step":"save"}]);
    for steps in [
        r#"{"restore":{"outcome":"skipped","outputs":{}}}"#,
        r#"{"restore":{"outcome":"success","outputs":{"cache-primary-key":"key","cache-hit":""}}}"#,
    ] {
        let metrics = report(&repo, &plan, steps)?;
        assert_eq!(metrics.caches[0].hit, velnor_ci::report::Hit::Miss);
        assert!(velnor_ci::report::check(&[metrics]).is_ok());
    }
    Ok(())
}
