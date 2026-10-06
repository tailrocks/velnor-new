use super::EntryCacheIds;
use velnor_actions_contract::canonical::digest_b3;

#[test]
fn cache_ids_need_five_valid_digests() {
    let good = digest_b3(b"workspace");
    let ids = EntryCacheIds::new(&good, &good, &good, &good, &good).expect("valid ids");
    assert_eq!(ids.workspace_id(), good);
    ids.validate().expect("revalidate");
    assert!(EntryCacheIds::new("b3-nope", &good, &good, &good, &good).is_err());
    let json = serde_json::to_string(&ids).expect("serialize");
    let back = serde_json::from_str::<EntryCacheIds>(&json).expect("deserialize");
    assert_eq!(back.cache_format_id(), good);
    let forged = json.replace(&good, "b3-nope");
    assert!(serde_json::from_str::<EntryCacheIds>(&forged).is_err());
}
