use super::super::*;
use super::{cache_key, mount, ready_seed, run, scratch};
use std::fs;

#[test]
fn matching_tool_seed_copies_both_trees_after_admission_and_keeps_source() {
    let root = scratch("hit");
    let seed = root.join("seed");
    let home = root.join("home");
    let key = cache_key();
    ready_seed(&seed, &key);
    let script = tool_seed_action_script(seed.to_str().expect("seed path")).expect("script");
    let output = run(&script, &home, &seed, &mount(&seed));
    assert!(output.status.success(), "{output:?}");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("tool seed restored share-dir"), "{text}");
    assert!(text.contains("tool seed restored toolchain-dir"), "{text}");
    assert_eq!(
        fs::read_to_string(home.join(".local/share/mise/installs/marker")).expect("mise copy"),
        "mise-bytes"
    );
    assert_eq!(
        fs::read_to_string(home.join("runner-temp/velnor/rustup/toolchains/marker"))
            .expect("rustup copy"),
        "rustup-bytes"
    );
    assert_eq!(
        fs::read_to_string(seed.join("mise/tree/installs/marker")).expect("seed kept"),
        "mise-bytes"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn rejected_mount_and_mismatched_key_leave_destinations_untouched() {
    let root = scratch("cold");
    let seed = root.join("seed");
    let home = root.join("home");
    ready_seed(&seed, &cache_key());
    let script = tool_seed_action_script(seed.to_str().expect("seed path")).expect("script");
    let writable = format!("{} ext4 0:77 rw", seed.display());
    let output = run(&script, &home, &seed, &writable);
    assert!(
        output.status.success(),
        "untrusted seed stays cold: {output:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("untrusted tool seed"),
        "{output:?}"
    );
    assert!(!home.exists(), "admission failure creates no destination");

    fs::write(seed.join("mise/KEY"), "mise-v1-other-key-0123456789abcdef").expect("wrong key");
    let output = run(&script, &home, &seed, &mount(&seed));
    assert!(
        output.status.success(),
        "key mismatch stays cold: {output:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("tool seed key mismatch"),
        "{output:?}"
    );
    assert!(!home.exists(), "key mismatch creates no destination");

    fs::write(seed.join("mise/KEY"), format!("{}\nextra\n", cache_key())).expect("multiline key");
    let output = run(&script, &home, &seed, &mount(&seed));
    assert!(
        output.status.success(),
        "multiline key stays cold: {output:?}"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("tool seed key mismatch"),
        "{output:?}"
    );
    assert!(!home.exists(), "multiline key creates no destination");
    fs::remove_dir_all(root).expect("cleanup");
}
