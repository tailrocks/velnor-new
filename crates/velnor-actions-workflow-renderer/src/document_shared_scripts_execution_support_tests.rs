use super::{DirectoryIdentity, new_root};
use std::fs;

#[test]
fn cleanup_refuses_a_replacement_directory_at_the_owned_path() {
    let mut owned = new_root().expect("create owned fixture");
    let mut holding = new_root().expect("create holding fixture");
    let original_path = owned.path().to_path_buf();
    let original_identity =
        DirectoryIdentity::capture(&original_path).expect("capture original fixture identity");
    let displaced_path = holding.path().join("displaced-original");
    fs::rename(&original_path, &displaced_path).expect("move original fixture");
    fs::create_dir(&original_path).expect("create replacement directory");
    let replacement_identity =
        DirectoryIdentity::capture(&original_path).expect("capture replacement identity");
    let sentinel = original_path.join("must-survive");
    fs::write(&sentinel, b"replacement-owned").expect("write replacement sentinel");

    let cleanup = owned.cleanup();
    assert!(cleanup.is_err(), "cleanup must reject a replaced path");
    assert_eq!(
        DirectoryIdentity::capture(&original_path).expect("replacement still exists"),
        replacement_identity
    );
    assert_eq!(
        fs::read(&sentinel).expect("replacement sentinel survives"),
        b"replacement-owned"
    );

    fs::remove_file(&sentinel).expect("remove replacement sentinel");
    fs::remove_dir(&original_path).expect("remove replacement directory");
    fs::rename(&displaced_path, &original_path).expect("restore original fixture");
    assert_eq!(
        DirectoryIdentity::capture(&original_path).expect("restored original exists"),
        original_identity
    );
    owned.cleanup().expect("remove restored owned fixture");
    holding.cleanup().expect("remove holding fixture");
}

#[test]
fn automatic_cleanup_waits_for_process_group_quiescence() {
    let mut owned = new_root().expect("create owned fixture");
    let path = owned.path().to_path_buf();
    let identity = DirectoryIdentity::capture(&path).expect("capture fixture identity");
    owned.mark_process_group_pending();
    assert!(
        owned.cleanup().is_err(),
        "explicit cleanup must reject unknown process-group state"
    );

    drop(owned);

    assert_eq!(
        DirectoryIdentity::capture(&path).expect("unconfirmed fixture is preserved"),
        identity
    );
    fs::remove_dir(&path).expect("remove checked preserved fixture");
}
