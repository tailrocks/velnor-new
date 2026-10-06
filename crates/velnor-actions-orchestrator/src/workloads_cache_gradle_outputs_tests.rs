use super::{build_logic_digest, eligibility, read_source_file, reviewed_file};
use sha2::{Digest, Sha256};

#[test]
fn unknown_repository_cannot_construct_output_contract() {
    let temp = tempfile::tempdir().expect("fixture");
    let index = velnor_actions_contract::build_index(temp.path(), &[]).expect("index");
    assert!(eligibility(&index, "backend").is_none());
    assert!(eligibility(&index, ".").is_none());
}

#[test]
fn oversized_shared_and_non_regular_sources_fail_before_reading() {
    let temp = tempfile::tempdir().expect("fixture");
    let path = temp.path().join("large.gradle");
    let file = std::fs::File::create(&path).expect("source");
    file.set_len(1_048_577).expect("oversized source");
    assert!(read_source_file(temp.path(), "large.gradle").is_none());
    assert!(build_logic_digest(temp.path()).is_none());
    std::fs::write(&path, "plugins {}").expect("small source");
    std::fs::hard_link(&path, temp.path().join("alias.gradle")).expect("hardlink");
    assert!(read_source_file(temp.path(), "large.gradle").is_none());
    assert!(read_source_file(temp.path(), "alias.gradle").is_none());
    #[cfg(unix)]
    {
        rustix::fs::mkfifoat(
            rustix::fs::CWD,
            temp.path().join("fifo.gradle"),
            rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
        )
        .expect("FIFO fixture");
        assert!(read_source_file(temp.path(), "fifo.gradle").is_none());
    }
}

#[test]
fn added_or_changed_unselected_build_script_changes_source_contract() {
    let temp = tempfile::tempdir().expect("fixture");
    let initial = build_logic_digest(temp.path()).expect("empty script inventory");
    std::fs::create_dir(temp.path().join("other-project")).expect("directory");
    let path = temp.path().join("other-project/build.gradle.kts");
    std::fs::write(&path, "tasks.configureEach { doFirst {} }").expect("script");
    let added = build_logic_digest(temp.path()).expect("script inventory");
    assert_ne!(initial, added);
    std::fs::write(path, "tasks.configureEach { doLast {} }").expect("changed script");
    assert_ne!(
        added,
        build_logic_digest(temp.path()).expect("changed inventory")
    );
}

#[test]
fn source_bytes_and_symlinks_cannot_forge_review() {
    let temp = tempfile::tempdir().expect("fixture");
    std::fs::write(temp.path().join("source.java"), "class Source {}").expect("source");
    let digest = format!("{:x}", Sha256::digest(b"class Source {}"));
    assert!(reviewed_file(temp.path(), "source.java", &digest));
    std::fs::write(temp.path().join("source.java"), "class Secret {}").expect("mutation");
    assert!(!reviewed_file(temp.path(), "source.java", &digest));
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("source.java", temp.path().join("alias.java")).expect("link");
        assert!(!reviewed_file(temp.path(), "alias.java", &digest));
        assert!(build_logic_digest(temp.path()).is_none());
    }
}
