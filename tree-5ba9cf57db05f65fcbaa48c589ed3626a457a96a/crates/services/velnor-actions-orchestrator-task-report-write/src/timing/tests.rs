use super::{elapsed_ms, now_ms, parse_exit_code, parse_start_ms};

#[test]
fn exit_codes_accept_the_eight_bit_range() {
    assert_eq!(parse_exit_code("0").expect("zero"), 0);
    assert_eq!(parse_exit_code("1").expect("one"), 1);
    assert_eq!(parse_exit_code("255").expect("max"), 255);
    for bad in ["", " ", "-1", "256", "99999", "0x1", "1\n", "ok"] {
        assert!(parse_exit_code(bad).is_err(), "must reject {bad:?}");
    }
}

#[test]
fn start_stamps_parse_and_measure_or_fail_closed() {
    assert_eq!(parse_start_ms("0"), Some(0));
    assert_eq!(parse_start_ms("1759270000000"), Some(1_759_270_000_000));
    assert_eq!(parse_start_ms("99999999999999999999999999"), None);
    for bad in ["", " ", "-1", "12.5", "0x1", "abc", "1\n"] {
        assert_eq!(parse_start_ms(bad), None, "must reject {bad:?}");
    }
    assert_eq!(elapsed_ms(None), None, "absent telemetry stays unmeasured");
    assert_eq!(
        elapsed_ms(Some(u64::MAX)),
        None,
        "future stamps stay unmeasured"
    );
    assert!(
        elapsed_ms(Some(1)).is_some_and(|elapsed| elapsed >= 1),
        "past stamps measure"
    );
    let now = now_ms().expect("wall clock");
    assert!(
        elapsed_ms(Some(now)).is_some_and(|elapsed| elapsed >= 1),
        "present stamps never read zero"
    );
}
