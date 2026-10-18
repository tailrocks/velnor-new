//! Runner-context parsing rejects run values outside the required form.

use super::{parse_positive_u32, parse_positive_u64, parse_ref_protected};

#[test]
fn protected_ref_accepts_only_runner_boolean_spellings() {
    assert!(parse_ref_protected("true").expect("true"));
    assert!(!parse_ref_protected("false").expect("false"));
    for value in ["", "TRUE", "0", "yes", " false "] {
        assert!(parse_ref_protected(value).is_err(), "reject {value:?}");
    }
}

#[test]
fn run_coordinates_require_positive_canonical_decimal_values() {
    assert_eq!(parse_positive_u64("72", "run_id").expect("run ID"), 72);
    assert_eq!(parse_positive_u32("3", "run_attempt").expect("attempt"), 3);
    for value in ["", "0", "072", "+72", "72 ", "1\n"] {
        assert!(
            parse_positive_u64(value, "run_id").is_err(),
            "reject {value:?}"
        );
        assert!(
            parse_positive_u32(value, "run_attempt").is_err(),
            "reject {value:?}"
        );
    }
}
