use super::tests::metadata;
use super::*;

#[test]
fn staged_manifest_is_exclusive_and_keeps_admitted_bytes() {
    let root = tempfile::tempdir().expect("staging root");
    let payload = br#"{"schema":2}"#;
    let archive = test_archive("baseline.json", payload);
    let metadata = metadata(99, &archive);
    let artifact_name = format!("velnor-baseline-{}-b3-{}", "a".repeat(40), "b".repeat(64));
    let path = stage_baseline_archive(root.path(), &artifact_name, &metadata, &archive)
        .expect("stage admitted archive");
    assert_eq!(std::fs::read(&path).expect("read staged bytes"), payload);
    assert!(stage_baseline_archive(root.path(), &artifact_name, &metadata, &archive).is_err());
    assert_eq!(
        std::fs::read(&path).expect("preserved staged bytes"),
        payload
    );
}
