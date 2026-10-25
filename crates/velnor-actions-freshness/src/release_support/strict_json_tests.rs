use super::parse_strict_json_bytes;

#[test]
fn maintenance_json_rejects_duplicate_escaped_keys_and_invalid_framing() {
    for bytes in [
        br#"{"a":1,"\u0061":2}"#.as_slice(),
        br#"{"nested":{"a":1,"a":2}}"#,
        b"{} {}",
        &[0xff],
    ] {
        assert!(parse_strict_json_bytes(bytes, 1024).is_err());
    }
    assert!(parse_strict_json_bytes(br#"{"a":1}"#, 7).is_ok());
    assert!(parse_strict_json_bytes(br#"{"a":1}"#, 6).is_err());
}

#[test]
fn maintenance_json_obeys_the_existing_container_depth_boundary() {
    let accepted = format!("{}0{}", "[".repeat(127), "]".repeat(127));
    let rejected = format!("{}0{}", "[".repeat(128), "]".repeat(128));
    assert!(parse_strict_json_bytes(accepted.as_bytes(), 1024).is_ok());
    assert!(parse_strict_json_bytes(rejected.as_bytes(), 1024).is_err());
}
