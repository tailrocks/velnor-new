use super::{parse_canonical_positive_decimal, parse_canonical_positive_decimal_u32};

#[test]
fn preview_manual_run_identity_requires_canonical_positive_decimal() {
    for invalid in [
        "",
        "0",
        "00",
        "01",
        "+1",
        " 1",
        "1 ",
        "18446744073709551616",
    ] {
        assert!(
            parse_canonical_positive_decimal(invalid).is_err(),
            "accepted noncanonical or out-of-range run id {invalid:?}"
        );
    }
    assert_eq!(
        parse_canonical_positive_decimal("1").expect("canonical run id"),
        1
    );
    assert_eq!(
        parse_canonical_positive_decimal("18446744073709551615").expect("maximum canonical run id"),
        u64::MAX
    );

    for invalid in ["0", "01", "4294967296"] {
        assert!(
            parse_canonical_positive_decimal_u32(invalid).is_err(),
            "accepted noncanonical or out-of-range attempt {invalid:?}"
        );
    }
    assert_eq!(
        parse_canonical_positive_decimal_u32("2").expect("canonical attempt"),
        2
    );
}
