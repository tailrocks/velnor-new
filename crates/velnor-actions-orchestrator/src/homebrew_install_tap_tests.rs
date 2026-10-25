use super::*;
use std::{fs, time::Duration};

fn deadline() -> CheckDeadline {
    CheckDeadline::after(Duration::from_secs(5)).expect("deadline")
}

fn fixture_root() -> tempfile::TempDir {
    let temp = std::env::temp_dir()
        .canonicalize()
        .expect("physical system temporary directory");
    let root = tempfile::tempdir_in(temp).expect("private fixture root");
    let library = root.path().join("opt/homebrew/Library");
    fs::create_dir_all(&library).expect("Homebrew Library");
    root
}

fn formula_directory(root: &Path) -> PathBuf {
    root.join("opt/homebrew/Library/Taps/velnor/homebrew-native-install/Formula")
}

fn digest(bytes: &[u8]) -> String {
    super::super::support::sha256(bytes)
}

#[test]
fn fresh_loader_is_private_stable_and_refuses_existing_tap() {
    let root = fixture_root();
    let path = formula_directory(root.path());
    let bytes = b"class ExamplePreview < Formula; end\n";
    let loader =
        TapFormula::create_fresh_at(&path, "example-preview", bytes, &digest(bytes), deadline())
            .expect("fresh tap loader");
    assert_eq!(loader.path, path.join("example-preview.rb"));
    assert_eq!(fs::read(&loader.path).expect("loader bytes"), bytes);
    assert_eq!(
        fs::metadata(&path)
            .expect("formula directory")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert!(
        TapFormula::create_fresh_at(&path, "example-preview", bytes, &digest(bytes), deadline())
            .is_err()
    );
}

#[test]
fn replacement_keeps_staged_canonical_a_and_installs_d_loader_atomically() {
    let root = fixture_root();
    let directory = formula_directory(root.path());
    let a = b"class ExamplePreview < Formula; url 'a'; end\n";
    let d = b"class ExamplePreview < Formula; url 'd'; end\n";
    let loader_a =
        TapFormula::create_fresh_at(&directory, "example-preview", a, &digest(a), deadline())
            .expect("A loader");
    let canonical_a = root.path().join("request-a/example-preview.rb");
    fs::create_dir(canonical_a.parent().expect("A parent")).expect("A staging");
    fs::write(&canonical_a, a).expect("A canonical formula");
    let loader_d = loader_a
        .replace(d, &digest(d), a, deadline())
        .expect("atomic D loader");
    loader_d.verify(deadline()).expect("held D loader");
    assert_eq!(fs::read(&loader_d.path).expect("D loader"), d);
    assert_eq!(fs::read(canonical_a).expect("A canonical remains"), a);
    assert!(!directory.join(".velnor-example-preview.stage").exists());
}

#[test]
fn replacement_refuses_changed_loader_and_symlinked_tap_ancestors() {
    let root = fixture_root();
    let directory = formula_directory(root.path());
    let a = b"A";
    let d = b"D";
    let loader =
        TapFormula::create_fresh_at(&directory, "example-preview", a, &digest(a), deadline())
            .expect("A loader");
    fs::write(&loader.path, b"foreign").expect("tamper stable loader");
    assert!(loader.replace(d, &digest(d), a, deadline()).is_err());

    let other = fixture_root();
    let symlink_root = fixture_root();
    let library = symlink_root.path().join("opt/homebrew/Library");
    fs::create_dir_all(&library).expect("Library");
    let taps = library.join("Taps");
    std::os::unix::fs::symlink(other.path(), &taps).expect("planted Taps symlink");
    assert!(
        TapFormula::create_fresh_at(
            &formula_directory(symlink_root.path()),
            "example-preview",
            a,
            &digest(a),
            deadline()
        )
        .is_err()
    );
}

#[test]
fn trust_identity_is_the_fixed_tap_and_formula() {
    assert_eq!(
        formula_reference("jackin-preview").expect("qualified tap identity"),
        "velnor/native-install/jackin-preview"
    );
    let long_name = "a".repeat(65);
    for invalid in ["../formula", "Upper", "1formula", long_name.as_str()] {
        assert!(formula_reference(invalid).is_err());
    }
    assert!(formula_reference("jackin--preview-").is_ok());
    assert_eq!(HOMEBREW_CANDIDATE_TAP, "velnor/native-install");
    assert!(HOMEBREW_CANDIDATE_FORMULAE.ends_with("/Taps/velnor/homebrew-native-install/Formula"));
}
