use super::copy_file_to_new;
use std::fs;

#[test]
fn failed_exclusive_copy_preserves_existing_destination() {
    let scratch = tempfile::tempdir().expect("scratch");
    let base = fs::canonicalize(scratch.path()).expect("canonical path");
    let source = base.join("source");
    let destination = base.join("destination");
    fs::write(&source, b"source bytes").expect("source");
    fs::write(&destination, b"foreign bytes").expect("destination");

    assert!(copy_file_to_new(&source, &destination).is_err());
    assert_eq!(
        fs::read(destination).expect("preserved destination"),
        b"foreign bytes"
    );
}
