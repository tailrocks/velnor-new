use super::parse_downstream;

#[test]
fn downstream_ids_split_dedupe_and_drop_blanks() {
    assert!(parse_downstream(None).is_empty());
    assert!(parse_downstream(Some("")).is_empty());
    assert_eq!(parse_downstream(Some("b,a,b,, a ,")), ["b", "a"]);
}
