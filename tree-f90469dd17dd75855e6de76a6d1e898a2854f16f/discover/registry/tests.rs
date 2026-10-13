use super::*;

/// The dispatch enum and the authoritative registry agree exactly.
#[test]
fn dispatch_enum_matches_registered_stacks() {
    let ids: Vec<&str> = Stack::all().iter().map(|stack| stack.id()).collect();
    assert_eq!(ids, crate::config::VelnorConfig::REGISTERED_STACKS);
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "registry runs ascending");
    assert_eq!(Stack::require_known("rust"), Ok(Stack::Rust));
    assert_eq!(Stack::require_known("tofu"), Ok(Stack::Tofu));
}
