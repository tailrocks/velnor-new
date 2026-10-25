use std::fs;

use velnor_actions_contract::NATIVE_IMAGE_CANDIDATE_INPUTS;

use super::source::source_closure;

const ARCHIVE_GUARD_BUILD_INPUTS: [&str; 3] = [
    "crates/velnor-archive-guard/build.rs",
    "crates/velnor-archive-guard/build_support/archive_guard_inputs.rs",
    "crates/velnor-archive-guard/build_support/archive_guard_inputs_filesystem.rs",
];

#[test]
fn source_closure_hash_changes_for_each_archive_guard_build_input() {
    for input in ARCHIVE_GUARD_BUILD_INPUTS {
        assert!(NATIVE_IMAGE_CANDIDATE_INPUTS.contains(&input));
    }

    let temporary = tempfile::tempdir().expect("temporary source root");
    for input in NATIVE_IMAGE_CANDIDATE_INPUTS {
        let path = temporary.path().join(input);
        fs::create_dir_all(path.parent().expect("source path parent"))
            .expect("create source parent");
        fs::write(&path, input.as_bytes()).expect("write source fixture");
    }
    let commit = "a".repeat(40);
    let baseline = source_closure(temporary.path(), &commit).expect("baseline closure");

    for input in ARCHIVE_GUARD_BUILD_INPUTS {
        let path = temporary.path().join(input);
        let original = input.as_bytes();
        let changed = [original, b"\nchanged"].concat();
        fs::write(&path, &changed).expect("change executable build input");
        let mutated = source_closure(temporary.path(), &commit).expect("mutated closure");
        assert_ne!(baseline.inputs[input], mutated.inputs[input], "{input}");
        assert_ne!(baseline, mutated, "{input}");
        fs::write(&path, original).expect("restore build input fixture");
    }
}
