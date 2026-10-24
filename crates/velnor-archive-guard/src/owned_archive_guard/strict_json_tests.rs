use super::parse;

#[test]
fn strict_json_rejects_duplicate_escaped_keys_non_finite_numbers_and_bad_framing() {
    for bytes in [
        br#"{"a":1,"\u0061":2}"#.as_slice(),
        br#"{"nested":{"a":1,"a":2}}"#,
        b"{} {}",
        b"1e9999",
        &[0xff],
    ] {
        assert!(parse(bytes).is_err());
    }
    assert!(parse(br#"{"a":1}"#).is_ok());
}
